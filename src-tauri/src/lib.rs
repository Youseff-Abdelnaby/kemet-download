use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

// قائمة العمليات الجارية: (رقم التحميل، رقم العملية في ويندوز)
static PROCS: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Opts {
    url: String,
    quality: String,      // best | 4320 | 2160 | 1440 | 1080 | 720 | 480 | 360 | audio
    container: String,    // mp4 | mkv
    codec: String,        // any | h264 | h265 | av1
    audio_format: String, // best | mp3 | m4a | opus | flac
    cookies: String,      // none | chrome | edge | firefox
    playlist: bool,
}

#[derive(Clone, serde::Serialize)]
struct Progress {
    id: u32,
    line: String,
    percent: Option<f32>,
}

// مكان الأدوات (yt-dlp, ffmpeg, deno) بجانب ملف التطبيق
fn tools_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default()
}

fn tool_path(name: &str) -> PathBuf {
    tools_dir().join(format!("{name}.exe"))
}

fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    #[cfg(not(windows))]
    let _ = cmd;
}

fn read_lines<R: Read>(r: R, mut f: impl FnMut(String)) {
    let mut br = BufReader::new(r);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        match br.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => f(String::from_utf8_lossy(&buf).trim().to_string()),
        }
    }
}

fn parse_percent(line: &str) -> Option<f32> {
    if !line.starts_with("[download]") {
        return None;
    }
    for tok in line.split_whitespace() {
        if let Some(num) = tok.strip_suffix('%') {
            if let Ok(v) = num.parse::<f32>() {
                return Some(v);
            }
        }
    }
    None
}

fn build_args(o: &Opts, out_dir: &Path) -> Vec<String> {
    let mut a: Vec<String> = [
        "--newline",
        "--no-colors",
        "--encoding",
        "utf-8",
        "--windows-filenames",
        "--retries",
        "infinite",
        "--fragment-retries",
        "infinite",
        "--retry-sleep",
        "3",
        "--socket-timeout",
        "30",
        "-N",
        "8",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    a.push("--ffmpeg-location".into());
    a.push(tools_dir().to_string_lossy().to_string());
    a.push("--js-runtimes".into());
    a.push(format!("deno:{}", tool_path("deno").to_string_lossy()));
    a.push("-P".into());
    a.push(out_dir.to_string_lossy().to_string());
    a.push("-o".into());
    a.push("%(title).150B [%(id)s].%(ext)s".into());

    if !o.playlist {
        a.push("--no-playlist".into());
    }
    if !o.cookies.is_empty() && o.cookies != "none" {
        a.push("--cookies-from-browser".into());
        a.push(o.cookies.clone());
    }

    if o.quality == "audio" {
        a.push("-x".into());
        a.push("--audio-format".into());
        a.push(o.audio_format.clone());
        a.push("--audio-quality".into());
        a.push("0".into());
    } else {
        let mut sort: Vec<String> = Vec::new();
        if o.quality != "best" {
            sort.push(format!("res:{}", o.quality));
        }
        match o.codec.as_str() {
            "h264" => sort.push("vcodec:h264".into()),
            "h265" => sort.push("vcodec:h265".into()),
            "av1" => sort.push("vcodec:av01".into()),
            _ => {}
        }
        if !sort.is_empty() {
            a.push("-S".into());
            a.push(sort.join(","));
        }
        a.push("--merge-output-format".into());
        a.push(o.container.clone());
    }
    a
}

fn kill_tree(pid: u32) {
    let mut c = Command::new("taskkill");
    c.args(["/PID", &pid.to_string(), "/T", "/F"]);
    hide_window(&mut c);
    let _ = c.status();
}

fn run_download(app: AppHandle, id: u32, o: Opts, out_dir: PathBuf) -> Result<String, String> {
    if !(o.url.starts_with("http://") || o.url.starts_with("https://")) {
        return Err("الرابط لازم يبدأ بـ http أو https".into());
    }
    let mut cmd = Command::new(tool_path("yt-dlp"));
    cmd.args(build_args(&o, &out_dir))
        .arg("--")
        .arg(&o.url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("تعذر تشغيل yt-dlp: {e}"))?;
    PROCS.lock().unwrap().push((id, child.id()));

    let stderr = child.stderr.take().unwrap();
    let err_thread = std::thread::spawn(move || {
        let mut last = String::new();
        read_lines(stderr, |l| {
            if l.contains("ERROR") {
                last = l;
            }
        });
        last
    });

    let stdout = child.stdout.take().unwrap();
    let app2 = app.clone();
    read_lines(stdout, |l| {
        if l.is_empty() {
            return;
        }
        let p = Progress {
            id,
            percent: parse_percent(&l),
            line: l,
        };
        let _ = app2.emit("progress", p);
    });

    let status = child.wait().map_err(|e| e.to_string())?;
    let last_err = err_thread.join().unwrap_or_default();

    // لو الـ pause شالت العملية من القائمة يبقى إحنا اللي وقفناها
    let was_paused = {
        let mut p = PROCS.lock().unwrap();
        let before = p.len();
        p.retain(|(i, _)| *i != id);
        before == p.len()
    };
    if was_paused {
        return Err("paused".into());
    }
    if status.success() {
        Ok("done".into())
    } else if last_err.is_empty() {
        Err("فشل التحميل (راجع الرابط أو الاتصال)".into())
    } else {
        Err(last_err)
    }
}

#[tauri::command]
async fn download(app: AppHandle, id: u32, opts: Opts) -> Result<String, String> {
    let out_dir = app
        .path()
        .download_dir()
        .map_err(|e| e.to_string())?
        .join("Kemet Download");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || run_download(app, id, opts, out_dir))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn pause(id: u32) {
    let pid = {
        let mut p = PROCS.lock().unwrap();
        let pos = p.iter().position(|(i, _)| *i == id);
        pos.map(|x| p.remove(x).1)
    };
    if let Some(pid) = pid {
        kill_tree(pid);
    }
}

#[tauri::command]
fn open_folder(app: AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .download_dir()
        .map_err(|e| e.to_string())?
        .join("Kemet Download");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Command::new("explorer")
        .arg(dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn update_engine() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut c = Command::new(tool_path("yt-dlp"));
        c.arg("-U").stdin(Stdio::null());
        hide_window(&mut c);
        let out = c.output().map_err(|e| e.to_string())?;
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok(text.trim().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            download,
            pause,
            open_folder,
            update_engine
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kemet Download");
}
