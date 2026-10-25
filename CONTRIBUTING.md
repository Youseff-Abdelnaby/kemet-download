# Contributing to Kemet Download / المساهمة في المشروع

Kemet Download is free and open source, and contributions are welcome.
المشروع مجاني ومفتوح المصدر، وأي مساهمة مرحّب بها.

## Ways to help / طرق المساعدة

- **Report a bug / بلاغ عن مشكلة:** open an [Issue](../../issues) with the link that failed (if it is not private), the quality/format you picked, and the error text shown in the app.
- **Suggest a feature / اقتراح ميزة:** open an Issue and describe what you need.
- **Improve the code / تعديل الكود:** fork the repo, make your change, and open a Pull Request.
- **Translate / ترجمة:** the UI lives in `src/index.html`.

## Project layout / هيكل المشروع

| Path | What it does |
|------|--------------|
| `src/index.html` | The whole user interface (HTML + CSS + JS, no build step). |
| `src-tauri/src/lib.rs` | Rust backend: runs yt-dlp, reports progress, pause/resume. |
| `src-tauri/tauri.conf.json` | Tauri app settings and bundled tools. |
| `.github/workflows/build.yml` | Builds the Windows installer on GitHub's servers. |

## Pull request checklist

1. Keep the change small and focused.
2. Describe what you changed and why.
3. Make sure the **Build Windows** check turns green.

## Code of conduct

Be respectful and helpful. Harassment and abuse are not tolerated.
