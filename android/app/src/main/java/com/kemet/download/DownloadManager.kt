package com.kemet.download

import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.io.RandomAccessFile
import java.net.URL
import java.net.URLDecoder
import java.util.concurrent.TimeUnit

enum class Status(val ar: String) {
    WAITING("في الانتظار"),
    DOWNLOADING("جاري التحميل"),
    PAUSED("متوقف مؤقتاً"),
    DONE("تم التحميل بنجاح 🏆"),
    ERROR("فشل")
}

data class Item(
    val id: Int,
    val url: String,
    val status: Status = Status.WAITING,
    val pct: Float = 0f,
    val line: String = "",
    val err: String = ""
)

object DownloadManager {
    val items = MutableStateFlow<List<Item>>(emptyList())
    @Volatile var parallel = 10

    private var nextId = 1
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val jobs = HashMap<Int, Job>()
    private const val UA =
        "Mozilla/5.0 (Linux; Android 13) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Mobile Safari/537.36"
    private val client = OkHttpClient.Builder()
        .connectTimeout(20, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .followRedirects(true)
        .followSslRedirects(true)
        .build()

    private fun update(id: Int, f: (Item) -> Item) =
        items.update { l -> l.map { if (it.id == id) f(it) else it } }

    @Synchronized
    fun addUrls(text: String): Int {
        var added = 0
        for (m in Regex("https?://[^\\s\"'<>]+").findAll(text)) {
            val u = m.value
            if (items.value.any { it.url == u && it.status != Status.DONE && it.status != Status.ERROR }) continue
            items.update { it + Item(nextId++, u) }
            added++
        }
        return added
    }

    fun begin(ctx: Context) {
        pump(ctx)
        ContextCompat.startForegroundService(ctx, Intent(ctx, DownloadService::class.java))
    }

    @Synchronized
    fun pump(ctx: Context) {
        val app = ctx.applicationContext
        var active = items.value.count { it.status == Status.DOWNLOADING }
        while (active < parallel) {
            val next = items.value.firstOrNull { it.status == Status.WAITING } ?: break
            update(next.id) { it.copy(status = Status.DOWNLOADING, err = "") }
            jobs[next.id] = scope.launch { run(app, next.id, next.url) }
            active++
        }
    }

    fun resume(ctx: Context, id: Int) {
        update(id) { it.copy(status = Status.WAITING) }
        begin(ctx)
    }

    @Synchronized fun pause(id: Int) { jobs[id]?.cancel() }

    @Synchronized
    fun pauseAll() {
        items.update { l -> l.map { if (it.status == Status.WAITING) it.copy(status = Status.PAUSED) else it } }
        jobs.values.forEach { it.cancel() }
    }

    @Synchronized
    fun remove(id: Int) {
        jobs[id]?.cancel()
        items.update { l -> l.filter { it.id != id } }
    }

    fun clearDone() = items.update { l -> l.filter { it.status != Status.DONE } }

    private suspend fun run(ctx: Context, id: Int, rawUrl: String) {
        try {
            val frag = rawUrl.substringAfter('#', "")
            val url = if (frag.startsWith("http")) frag else rawUrl
            download(ctx, id, url)
            update(id) { it.copy(status = Status.DONE, pct = 100f) }
        } catch (e: CancellationException) {
            update(id) { if (it.status == Status.DOWNLOADING) it.copy(status = Status.PAUSED) else it }
        } catch (e: Exception) {
            update(id) { it.copy(status = Status.ERROR, err = e.message ?: e.toString()) }
        } finally {
            synchronized(this) { jobs.remove(id) }
            pump(ctx)
        }
    }

    private suspend fun download(ctx: Context, id: Int, url: String) {
        val base = ctx.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS) ?: ctx.filesDir
        val dir = File(base, "Kemet Download").apply { mkdirs() }
        val part = File(dir, ".${fnv(url)}.part")
        var attempt = 0
        while (true) {
            try {
                downloadOnce(ctx, id, url, dir, part)
                return
            } catch (e: IOException) {
                currentCoroutineContext().ensureActive()
                if (++attempt > 5) throw e
                update(id) { it.copy(line = "انقطع الاتصال.. إعادة المحاولة $attempt/5") }
                delay(2000)
            }
        }
    }

    private suspend fun downloadOnce(ctx: Context, id: Int, url: String, dir: File, part: File) {
        var start = if (part.exists()) part.length() else 0L
        val req = Request.Builder().url(url)
            .header("User-Agent", UA)
            .header("Referer", origin(url))
            .apply { if (start > 0) header("Range", "bytes=$start-") }
            .build()

        client.newCall(req).execute().use { resp ->
            val disp = resp.header("Content-Disposition")
            if (resp.code == 416 && start > 0) {
                publish(ctx, part, dir, fileName(disp, url))
                return
            }
            if (resp.code >= 500) throw IOException("HTTP ${resp.code}")
            if (!resp.isSuccessful) throw IllegalStateException("HTTP ${resp.code}")
            val ct = resp.header("Content-Type").orEmpty()
            if (ct.startsWith("text/html") && disp == null)
                throw IllegalStateException("الرابط صفحة ويب وليس ملفاً مباشراً")

            val name = fileName(disp, url)
            if (start > 0 && resp.code != 206) start = 0
            val body = resp.body ?: throw IOException("empty body")
            val total: Long? = if (resp.code == 206)
                resp.header("Content-Range")?.substringAfter('/')?.toLongOrNull()
            else body.contentLength().takeIf { it > 0 }

            RandomAccessFile(part, "rw").use { raf ->
                if (start == 0L) raf.setLength(0)
                raf.seek(start)
                val buf = ByteArray(64 * 1024)
                val src = body.byteStream()
                var done = start
                var lastT = System.nanoTime()
                var lastB = done
                var speed = 0.0
                while (true) {
                    val n = src.read(buf)
                    if (n < 0) break
                    raf.write(buf, 0, n)
                    done += n
                    currentCoroutineContext().ensureActive()
                    val now = System.nanoTime()
                    if (now - lastT > 500_000_000L) {
                        val inst = (done - lastB) / ((now - lastT) / 1e9)
                        speed = if (speed == 0.0) inst else speed * 0.7 + inst * 0.3
                        lastT = now; lastB = done
                        val pct = if (total != null) done * 100f / total else 0f
                        val eta = if (total != null && speed > 1) fmtEta(((total - done) / speed).toLong()) else "--:--:--"
                        val line = "${hb(done.toDouble())} / ${total?.let { hb(it.toDouble()) } ?: "?"} — ${hb(speed)}/s — ETA $eta"
                        update(id) { it.copy(pct = pct, line = line) }
                    }
                }
                if (total != null && done < total) throw IOException("انقطع الاتصال")
            }
            val where = publish(ctx, part, dir, name)
            update(id) { it.copy(line = "تم الحفظ: $where") }
        }
    }

    private fun publish(ctx: Context, part: File, dir: File, name: String): String {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val v = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, name)
                put(MediaStore.Downloads.RELATIVE_PATH, "${Environment.DIRECTORY_DOWNLOADS}/Kemet Download")
                put(MediaStore.Downloads.IS_PENDING, 1)
            }
            val r = ctx.contentResolver
            val uri = r.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, v)
                ?: throw IllegalStateException("تعذر إنشاء الملف في التنزيلات")
            r.openOutputStream(uri)!!.use { o -> part.inputStream().use { it.copyTo(o) } }
            v.clear()
            v.put(MediaStore.Downloads.IS_PENDING, 0)
            r.update(uri, v, null, null)
            part.delete()
            return "Download/Kemet Download/$name"
        }
        val ext = if (name.contains('.')) "." + name.substringAfterLast('.') else ""
        val stem = name.removeSuffix(ext)
        var t = File(dir, name)
        var i = 1
        while (t.exists()) t = File(dir, "$stem (${i++})$ext")
        part.renameTo(t)
        return t.absolutePath
    }

    private fun dec(s: String) = runCatching { URLDecoder.decode(s.trim('"'), "UTF-8") }.getOrDefault(s)

    private fun fileName(disp: String?, url: String): String {
        val fromDisp = disp?.let { d ->
            Regex("filename\\*=(?:UTF-8|utf-8)''([^;]+)").find(d)?.groupValues?.get(1)?.let { dec(it) }
                ?: Regex("filename=\"?([^\";]+)\"?").find(d)?.groupValues?.get(1)
        }
        val raw = fromDisp?.takeIf { it.isNotBlank() }
            ?: dec(url.substringBefore('#').substringBefore('?').substringAfterLast('/'))
        val clean = raw.replace(Regex("[\\\\/:*?\"<>|\\p{Cntrl}]"), "_").take(180).trim().trim('.')
        return clean.ifEmpty { "download.bin" }
    }

    private fun origin(url: String) =
        runCatching { URL(url).let { "${it.protocol}://${it.host}/" } }.getOrDefault(url)

    private fun fnv(s: String): String {
        var h = -0x340d631b7bdddcdbL
        for (b in s.toByteArray()) { h = h xor (b.toLong() and 0xff); h *= 0x100000001b3L }
        return java.lang.Long.toHexString(h)
    }

    private fun hb(b: Double): String {
        val u = arrayOf("B", "KB", "MB", "GB", "TB")
        var v = b; var i = 0
        while (v >= 1024 && i < 4) { v /= 1024; i++ }
        return "%.1f %s".format(v, u[i])
    }

    private fun fmtEta(s: Long) = "%02d:%02d:%02d".format(s / 3600, (s % 3600) / 60, s % 60)
}
