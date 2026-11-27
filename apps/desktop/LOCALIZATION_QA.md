# CellX localization visual QA

Run the desktop app at 1024×768, 1280×800, and 1920×1080. Repeat the checklist for ko-KR, en-US, ja-JP, zh-CN, and zh-TW.

## Required visual checks

- Ribbon tab labels remain readable; the ribbon tab strip scrolls horizontally instead of clipping.
- Ribbon groups do not overlap; long button captions wrap within their button.
- Context menus and dropdowns stay inside the viewport and long captions wrap.
- Dialog titles, labels, inputs, and action buttons remain visible; dialogs scroll vertically when needed.
- Format Cells tabs and the format grid remain usable at the narrow viewport.
- Data-sheet controls and status-bar messages do not push the grid off-screen.
- Title-bar document name truncates with an ellipsis without covering the language selector.
- CJK glyphs use the appropriate installed font fallback and do not render as missing-glyph boxes.

## CSV locale checks

- Import UTF-8, UTF-8-BOM and UTF-16-BOM CSV files with auto-detected comma/semicolon delimiters.
- Import TSV files with tab delimiters.
- Confirm the selected UI locale is used as the preferred date convention.
- Export CSV and confirm UTF-8 BOM, header row, locale-specific date format, and decimal point.
- Export/import Parquet and confirm the locale has no effect on Parquet values.
- Confirm UTF-32 BOM files fail with a clear encoding message. UTF-8 and UTF-16 are supported; legacy encodings such as CP949 require conversion to UTF-8 before import.

## Automated coverage

locale-layout.test.mjs checks that the actual stylesheet retains the layout safeguards. It is a CSS regression test, not a substitute for rendering screenshots and checking the application in each locale.