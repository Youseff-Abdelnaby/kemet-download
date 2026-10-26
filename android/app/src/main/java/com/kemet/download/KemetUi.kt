package com.kemet.download

import android.app.DownloadManager as SysDownloads
import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val Bg = Color(0xFF12100C)
private val Panel = Color(0xFF1C1913)
private val Line = Color(0xFF3F3522)
private val Txt = Color(0xFFF3ECDA)
private val Muted = Color(0xFFA79D84)
private val Gold = Color(0xFFD9A93A)
private val Gold2 = Color(0xFFF0C75E)
private val Ok = Color(0xFF58B368)
private val Err = Color(0xFFE5645A)

@Composable
fun KemetApp() {
    val ctx = LocalContext.current
    val items by DownloadManager.items.collectAsState()
    var text by remember { mutableStateOf("") }
    var parallel by remember { mutableIntStateOf(DownloadManager.parallel) }

    val picker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        uri?.let { u ->
            ctx.contentResolver.openInputStream(u)?.bufferedReader()?.use { DownloadManager.addUrls(it.readText()) }
        }
    }

    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Gold, background = Bg, surface = Panel, onSurface = Txt, onBackground = Txt
        )
    ) {
        CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Rtl) {
            Surface(color = Bg, modifier = Modifier.fillMaxSize()) {
                LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    item {
                        Row(verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                            Image(
                                painterResource(R.drawable.kemet_logo), null,
                                Modifier.size(48.dp).clip(RoundedCornerShape(10.dp))
                            )
                            Column(Modifier.padding(start = 12.dp)) {
                                Text("Kemet Download", color = Gold2, fontSize = 22.sp,
                                    fontWeight = FontWeight.Bold, fontFamily = FontFamily.Serif)
                                Text("محمّل الوسائط الفرعوني — بدون إعلانات", color = Muted, fontSize = 12.sp)
                            }
                        }
                    }
                    item {
                        Card {
                            OutlinedTextField(
                                value = text, onValueChange = { text = it },
                                modifier = Modifier.fillMaxWidth(), minLines = 4, maxLines = 8,
                                placeholder = { Text("الصق الروابط هنا (رابط في كل سطر)", color = Muted, fontSize = 13.sp) },
                                textStyle = TextStyle(
                                    fontFamily = FontFamily.Monospace, fontSize = 13.sp,
                                    textDirection = TextDirection.Ltr, color = Txt
                                ),
                                colors = OutlinedTextFieldDefaults.colors(
                                    focusedBorderColor = Gold2, unfocusedBorderColor = Gold,
                                    focusedContainerColor = Color(0xFF0F0D0A),
                                    unfocusedContainerColor = Color(0xFF0F0D0A)
                                )
                            )
                            Text("تحميلات متزامنة", color = Muted, fontSize = 12.sp, modifier = Modifier.padding(top = 12.dp))
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                listOf(1, 2, 5, 10).forEach { n ->
                                    FilterChip(
                                        selected = parallel == n,
                                        onClick = { parallel = n; DownloadManager.parallel = n },
                                        label = { Text("$n") },
                                        colors = FilterChipDefaults.filterChipColors(
                                            selectedContainerColor = Gold, selectedLabelColor = Bg,
                                            labelColor = Txt
                                        )
                                    )
                                }
                            }
                            Row(
                                Modifier.padding(top = 12.dp),
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                Button(
                                    onClick = {
                                        DownloadManager.addUrls(text); text = ""
                                        DownloadManager.begin(ctx)
                                    },
                                    colors = ButtonDefaults.buttonColors(containerColor = Gold, contentColor = Bg)
                                ) { Text("⚡ ابدأ التحميل", fontWeight = FontWeight.Bold) }
                                Ghost("📁 ملف .txt") { picker.launch("text/plain") }
                                Ghost("📂 التنزيلات") {
                                    ctx.startActivity(
                                        Intent(SysDownloads.ACTION_VIEW_DOWNLOADS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                                    )
                                }
                            }
                        }
                    }
                    item {
                        Row(
                            Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = androidx.compose.ui.Alignment.CenterVertically
                        ) {
                            Text("قائمة التحميل النشطة", color = Txt, fontWeight = FontWeight.Bold)
                            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                Ghost("إيقاف الكل") { DownloadManager.pauseAll() }
                                Ghost("مسح المنتهي") { DownloadManager.clearDone() }
                            }
                        }
                    }
                    if (items.isEmpty()) {
                        item {
                            Text("لسه مفيش روابط في القائمة. الصق روابطك أو شاركها للتطبيق!",
                                color = Muted, modifier = Modifier.fillMaxWidth().padding(20.dp))
                        }
                    }
                    items(items, key = { it.id }) { ItemCard(it) }
                }
            }
        }
    }
}

@Composable
private fun Card(content: @Composable () -> Unit) {
    Surface(
        color = Panel, shape = RoundedCornerShape(12.dp), border = BorderStroke(1.dp, Line),
        modifier = Modifier.fillMaxWidth()
    ) { Column(Modifier.padding(16.dp)) { content() } }
}

@Composable
private fun Ghost(label: String, onClick: () -> Unit) {
    OutlinedButton(
        onClick = onClick, border = BorderStroke(1.dp, Line),
        colors = ButtonDefaults.outlinedButtonColors(contentColor = Txt),
        contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 10.dp, vertical = 4.dp)
    ) { Text(label, fontSize = 12.sp) }
}

@Composable
private fun ItemCard(it: Item) {
    val ctx = LocalContext.current
    Card {
        Text(
            it.url, color = Muted, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
            fontFamily = FontFamily.Monospace, style = TextStyle(textDirection = TextDirection.Ltr),
            modifier = Modifier.fillMaxWidth()
        )
        Row(Modifier.padding(vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            if (it.status == Status.DOWNLOADING) Ghost("إيقاف مؤقت") { DownloadManager.pause(it.id) }
            if (it.status == Status.PAUSED || it.status == Status.ERROR) {
                Button(
                    onClick = { DownloadManager.resume(ctx, it.id) },
                    colors = ButtonDefaults.buttonColors(containerColor = Gold, contentColor = Bg),
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 10.dp, vertical = 4.dp)
                ) { Text(if (it.status == Status.ERROR) "إعادة المحاولة" else "استئناف", fontSize = 12.sp) }
            }
            Ghost("حذف") { DownloadManager.remove(it.id) }
        }
        LinearProgressIndicator(
            progress = { (it.pct / 100f).coerceIn(0f, 1f) },
            modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(6.dp)),
            color = Gold2, trackColor = Color(0xFF0F0D0A)
        )
        val c = when (it.status) { Status.DONE -> Ok; Status.ERROR -> Err; else -> Muted }
        Text("${it.status.ar} — ${"%.1f".format(it.pct)}%", color = c, fontSize = 12.sp, modifier = Modifier.padding(top = 6.dp))
        val info = if (it.status == Status.ERROR) it.err else it.line
        if (info.isNotBlank()) {
            Text(info, color = Muted, fontSize = 11.sp, maxLines = 2, overflow = TextOverflow.Ellipsis,
                style = TextStyle(textDirection = TextDirection.Ltr), modifier = Modifier.fillMaxWidth())
        }
    }
}
