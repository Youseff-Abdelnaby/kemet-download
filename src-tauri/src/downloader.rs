use futures_util::StreamExt;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_RANGE, CONTENT_TYPE, RANGE, REFERER};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use tokio_util::sync::CancellationToken;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

#[derive(Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Opts {
    pub url: String,
    pub quality: String,
    pub container: String,
    pub codec: String,
    pub audio_format: String,
    pub cookies: String,
    pub playlist: bool,
}

#[derive(Serialize, Clone)]
struct Progress {
    id: u32,
    percent: Option<f64>,
    line: String,
}

fn emit(app: &AppHandle, id: u32, percent: Option<f64>, line: String) {
    let _ = app.emit("progress", Progress { id, percent, line });
}

pub struct Engine {
    client: reqwest::Client,
    tokens: Mutex<HashMap<u32, CancellationToken>>,
}

impl Engine {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(UA)
            .connect_timeout(Duration::from_secs(20))
            .build()
            .expect("http client");
        Self { client, tokens: Mutex::new(HashMap::new()) }
    }

    pub fn pause(&self, id: u32) {
        let t = self.tokens.lock().unwrap().remove(&id);
        if let Some(t) = t {
            t.cancel();
        }
    }

    pub async fn run(&self, app: AppHandle, id: u32, opts: Opts) -> Result<(), String> {
        let token = CancellationToken::new();
        self.tokens.lock().unwrap().insert(id, token.clone());
        let r = self.run_inner(&app, id, &opts, &token).await;
        self.tokens.lock().unwrap().remove(&id);
        r
    }

    async fn run_inner(&self, app: &AppHandle, id: u32, opts: &Opts, token: &CancellationToken) -> Result<(), String> {
        let url = resolve_url(&opts.url);
        let dir = downloads_dir();
        fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
        emit(app, id, None, "جاري الفحص...".into());
        if self.direct(app, id, &url, &dir, token).await? {
            Ok(())
        } else {
            self.ytdlp(app, id, opts, &url, &dir, token).await
        }
    }

    // ---------- تحميل مباشر (ألعاب / ZIP / RAR / أي ملف) مع استئناف ----------
    async fn direct(&self, app: &AppHandle, id: u32, url: &str, dir: &PathBuf, token: &CancellationToken) -> Result<bool, String> {
        let part = dir.join(format!(".kemet-{:016x}.part", fnv(url)));
        let mut attempt = 0;
        loop {
            match self.direct_once(app, id, url, dir, &part, token).await {
                Ok(v) => return Ok(v),
                Err(e) if e.starts_with("net:") && attempt < 5 => {
                    attempt += 1;
                    emit(app, id, None, format!("انقطع الاتصال.. إعادة المحاولة {attempt}/5"));
                    tokio::select! {
                        _ = token.cancelled() => return Err("paused".into()),
                        _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    }
                }
                Err(e) => return Err(e.trim_start_matches("net:").to_string()),
            }
        }
    }

    async fn direct_once(
        &self,
        app: &AppHandle,
        id: u32,
        url: &str,
        dir: &PathBuf,
        part: &PathBuf,
        token: &CancellationToken,
    ) -> Result<bool, String> {
        let mut start = fs::metadata(part).await.map(|m| m.len()).unwrap_or(0);
        let mut req = self.client.get(url).header(REFERER, origin(url));
        if start > 0 {
            req = req.header(RANGE, format!("bytes={start}-"));
        }
        let resp = tokio::select! {
            _ = token.cancelled() => return Err("paused".into()),
            r = req.send() => r.map_err(|e| format!("net:{e}"))?,
        };
        let status = resp.status();
        let disp = resp.headers().get(CONTENT_DISPOSITION).and_then(|v| v.to_str().ok()).map(str::to_string);
        let ctype = resp.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();

        // الملف مكتمل مسبقاً
        if status.as_u16() == 416 && start > 0 {
            let name = file_name(disp.as_deref(), url);
            let target = finalize(part, dir, &name).await?;
            emit(app, id, Some(100.0), format!("تم الحفظ: {}", target.display()));
            return Ok(true);
        }
        if status.is_server_error() {
            return Err(format!("net:HTTP {status}"));
        }
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        // صفحة ويب وليست ملفاً → yt-dlp
        if ctype.starts_with("text/html") && disp.is_none() {
            return Ok(false);
        }

        let name = file_name(disp.as_deref(), url);
        if start > 0 && status.as_u16() != 206 {
            start = 0; // الخادم تجاهل Range
        }
        let total = if status.as_u16() == 206 {
            resp.headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.rsplit('/').next())
                .and_then(|s| s.parse::<u64>().ok())
        } else {
            resp.content_length()
        };

        let open = if start > 0 {
            fs::OpenOptions::new().append(true).open(part).await
        } else {
            fs::File::create(part).await
        };
        let mut file = open.map_err(|e| e.to_string())?;

        let mut done = start;
        let mut stream = resp.bytes_stream();
        let mut last = Instant::now();
        let mut last_done = done;
        let mut speed = 0f64;

        loop {
            let chunk = tokio::select! {
                _ = token.cancelled() => {
                    let _ = file.flush().await;
                    return Err("paused".into());
                }
                c = stream.next() => c,
            };
            let Some(chunk) = chunk else { break };
            let bytes = chunk.map_err(|e| format!("net:{e}"))?;
            file.write_all(&bytes).await.map_err(|e| e.to_string())?;
            done += bytes.len() as u64;

            let el = last.elapsed();
            if el >= Duration::from_millis(300) {
                let inst = (done - last_done) as f64 / el.as_secs_f64();
                speed = if speed == 0.0 { inst } else { speed * 0.7 + inst * 0.3 };
                last = Instant::now();
                last_done = done;
                let pct = total.map(|t| done as f64 * 100.0 / t as f64);
                let eta = match (total, speed > 1.0) {
                    (Some(t), true) => fmt_eta(((t - done) as f64 / speed) as u64),
                    _ => "--:--:--".into(),
                };
                let tot = total.map(|t| hb(t as f64)).unwrap_or_else(|| "?".into());
                emit(app, id, pct, format!("{} / {} — {}/s — ETA {}", hb(done as f64), tot, hb(speed), eta));
            }
        }
        file.flush().await.map_err(|e| e.to_string())?;
        drop(file);

        if let Some(t) = total {
            if done < t {
                return Err("net:انقطع الاتصال قبل اكتمال الملف".into());
            }
        }
        let target = finalize(part, dir, &name).await?;
        emit(app, id, Some(100.0), format!("تم الحفظ: {}", target.display()));
        Ok(true)
    }

    // ---------- yt-dlp للفيديو والصوت ----------
    async fn ytdlp(&self, app: &AppHandle, id: u32, o: &Opts, url: &str, dir: &PathBuf, token: &CancellationToken) -> Result<(), String> {
        let use_cookies = matches!(o.cookies.as_str(), "chrome" | "edge" | "firefox");
        match run_ytdlp(app, id, o, url, dir, token, use_cookies).await {
            Err(e) if use_cookies && e.to_lowercase().contains("cookie") => {
                emit(app, id, None, "تعذّر قراءة كوكيز المتصفح.. إعادة المحاولة بدون كوكيز".into());
                run_ytdlp(app, id, o, url, dir, token, false).await
            }
            r => r,
        }
    }

    pub async fn update_engine(&self) -> Result<String, String> {
        let dir = bin_dir();
        fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
        let (asset, local) = if cfg!(windows) {
            ("yt-dlp.exe", "yt-dlp.exe")
        } else if cfg!(target_os = "macos") {
            ("yt-dlp_macos", "yt-dlp")
        } else {
            ("yt-dlp", "yt-dlp")
        };
        let url = format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{asset}");
        let bytes = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("{local}.new"));
        fs::write(&tmp, &bytes).await.map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
        }
        fs::rename(&tmp, dir.join(local)).await.map_err(|e| e.to_string())?;
        Ok(format!("✅ تم تحديث محرك التحميل ({} MB)", bytes.len() / 1_048_576))
    }
}

async fn run_ytdlp(
    app: &AppHandle,
    id: u32,
    o: &Opts,
    url: &str,
    dir: &PathBuf,
    token: &CancellationToken,
    cookies: bool,
) -> Result<(), String> {
    let mut a: Vec<String> = Vec::new();
    a.extend(
        [
            "--newline", "--no-colors", "--windows-filenames", "--no-mtime", "--continue",
            "--concurrent-fragments", "8", "--retries", "10", "--fragment-retries", "10",
        ]
        .map(String::from),
    );
    a.push("-P".into());
    a.push(dir.to_string_lossy().into_owned());
    a.push("-o".into());
    a.push("%(title).150B [%(id)s].%(ext)s".into());

    let q = o.quality.as_str();
    if q == "audio" {
        let af = match o.audio_format.as_str() {
            "mp3" => "mp3",
            "best" | "" | "zip" => "best",
            x => x,
        };
        a.extend(["-x", "--audio-format", af, "-f", "ba/b"].map(String::from));
    } else {
        let f = if q == "best" || q.is_empty() {
            "bv*+ba/b".to_string()
        } else {
            format!("bv*[height<={q}]+ba/b[height<={q}]/b")
        };
        a.push("-f".into());
        a.push(f);
        let c = if o.container == "mkv" { "mkv" } else { "mp4" };
        a.extend(["--merge-output-format", c].map(String::from));
        match o.codec.as_str() {
            "h264" => a.extend(["-S", "vcodec:h264"].map(String::from)),
            "h265" => a.extend(["-S", "vcodec:h265"].map(String::from)),
            "av1" => a.extend(["-S", "vcodec:av1"].map(String::from)),
            _ => {}
        }
    }
    a.push(if o.playlist { "--yes-playlist" } else { "--no-playlist" }.into());
    if cookies {
        a.push("--cookies-from-browser".into());
        a.push(o.cookies.clone());
    }
    let bd = bin_dir();
    if bd.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }).exists() {
        a.push("--ffmpeg-location".into());
        a.push(bd.to_string_lossy().into_owned());
    }
    a.push(url.to_string());

    let mut cmd = Command::new(ytdlp_path());
    cmd.args(&a)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            String::from("محرك التحميل (yt-dlp) غير موجود — اضغط «تحديث محرك التحميل» أولاً")
        } else {
            e.to_string()
        }
    })?;

    let last_err = Arc::new(Mutex::new(String::new()));
    let se = child.stderr.take().unwrap();
    let le = last_err.clone();
    let h = tokio::spawn(async move {
        let mut l = BufReader::new(se).lines();
        while let Ok(Some(line)) = l.next_line().await {
            if line.contains("ERROR") {
                *le.lock().unwrap() = line;
            }
        }
    });

    let so = child.stdout.take().unwrap();
    let mut lines = BufReader::new(so).lines();
    loop {
        tokio::select! {
            _ = token.cancelled() => {
                let _ = child.kill().await;
                return Err("paused".into());
            }
            l = lines.next_line() => match l {
                Ok(Some(line)) => {
                    let pct = if line.starts_with("[download]") {
                        line.split_whitespace()
                            .find(|t| t.ends_with('%'))
                            .and_then(|t| t.trim_end_matches('%').parse::<f64>().ok())
                    } else { None };
                    emit(app, id, pct, line.trim().to_string());
                }
                _ => break,
            }
        }
    }
    let st = child.wait().await.map_err(|e| e.to_string())?;
    let _ = h.await;
    if st.success() {
        emit(app, id, Some(100.0), "تم الحفظ في مجلد Kemet Download".into());
        Ok(())
    } else {
        let e = last_err.lock().unwrap().clone();
        Err(if e.is_empty() { format!("yt-dlp: {st}") } else { e })
    }
}

// ---------- أدوات مساعدة ----------
fn resolve_url(raw: &str) -> String {
    if let Ok(u) = reqwest::Url::parse(raw) {
        if let Some(f) = u.fragment() {
            if f.starts_with("http://") || f.starts_with("https://") {
                return f.to_string();
            }
        }
    }
    raw.to_string()
}

fn origin(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|u| format!("{}://{}/", u.scheme(), u.host_str().unwrap_or("")))
        .unwrap_or_default()
}

fn fnv(s: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn dec(s: &str) -> String {
    urlencoding::decode(s).map(|c| c.into_owned()).unwrap_or_else(|_| s.to_string())
}

fn file_name(disp: Option<&str>, url: &str) -> String {
    let mut raw: Option<String> = None;
    if let Some(d) = disp {
        let low = d.to_lowercase();
        if let Some(i) = low.find("filename*=") {
            let v = d[i + 10..].split(';').next().unwrap_or("").trim().trim_matches('"');
            let v = v.splitn(3, '\'').last().unwrap_or(v);
            raw = Some(dec(v));
        } else if let Some(i) = low.find("filename=") {
            let v = d[i + 9..].split(';').next().unwrap_or("").trim().trim_matches('"');
            raw = Some(v.to_string());
        }
    }
    let raw = raw.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.path_segments().and_then(|s| s.last().map(dec)))
            .unwrap_or_default()
    });
    let clean: String = raw
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
        .take(180)
        .collect();
    let clean = clean.trim().trim_matches('.').to_string();
    if clean.is_empty() { "download.bin".into() } else { clean }
}

fn unique(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut i = 1;
    loop {
        let c = p.with_file_name(format!("{stem} ({i}){ext}"));
        if !c.exists() {
            return c;
        }
        i += 1;
    }
}

async fn finalize(part: &PathBuf, dir: &PathBuf, name: &str) -> Result<PathBuf, String> {
    let target = unique(dir.join(name));
    fs::rename(part, &target).await.map_err(|e| e.to_string())?;
    Ok(target)
}

fn hb(b: f64) -> String {
    let u = ["B", "KB", "MB", "GB", "TB"];
    let (mut v, mut i) = (b, 0);
    while v >= 1024.0 && i < 4 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", u[i])
}

fn fmt_eta(s: u64) -> String {
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

fn downloads_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Kemet Download")
}

fn bin_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("Kemet Download").join("bin")
}

fn ytdlp_path() -> PathBuf {
    let p = bin_dir().join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" });
    if p.exists() { p } else { PathBuf::from("yt-dlp") }
}

pub fn open_dir() -> Result<(), String> {
    let d = downloads_dir();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let prog = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(prog).arg(&d).spawn().map(|_| ()).map_err(|e| e.to_string())
}
