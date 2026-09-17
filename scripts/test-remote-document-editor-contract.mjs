import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = (relativePath) => readFile(path.join(ROOT, relativePath), "utf8");

const dialog = await source("src/components/FileEditor/RemoteDocumentDialog.tsx");
const dialogStyles = await source("src/components/FileEditor/RemoteDocumentDialog.module.css");
const editor = await source("src/components/FileEditor/views/TextEditor.tsx");

// Dirty/clean state changes must never rerun an autofocus effect and steal the caret.
assert.match(
  dialog,
  /useEffect\(\(\) => \{\s*if \(!visible\) return;[\s\S]{0,260}requestAnimationFrame\(\(\) => dialogRef\.current\?\.focus\(\)\)[\s\S]{0,360}\}, \[visible\]\);/,
  "dialog autofocus must depend only on visibility",
);
assert.doesNotMatch(
  dialog,
  /querySelector<HTMLElement>\(['"]button:not\(:disabled\), select:not\(:disabled\), textarea:not\(:disabled\)['"]\)[\s\S]{0,80}focus\(\)/,
  "opening the editor must not focus whichever action happens to be first enabled",
);
assert.match(dialog, /<TextEditor[\s\S]{0,180}\bautoFocus\b/);

// Editor-owned keys run before the dialog focus trap. Plain Tab indents; Shift+Tab remains navigation.
assert.match(dialog, /if \(event\.defaultPrevented\) return;/);
assert.match(dialog, /document\.addEventListener\("keydown", handler\);/);
assert.doesNotMatch(dialog, /document\.addEventListener\("keydown", handler, true\)/);
assert.match(editor, /event\.key !== "Tab"[\s\S]{0,120}event\.shiftKey/);
assert.match(editor, /event\.preventDefault\(\);[\s\S]{0,180}const next = `\$\{value\.slice\(0, start\)\}\\t/);
assert.match(editor, /event\.key\.toLowerCase\(\) === "s"[\s\S]{0,100}event\.preventDefault\(\);[\s\S]{0,80}onSave\(\)/);

// Modal lifecycle restores the opener, and dirty edits do not churn the document-level listener.
assert.match(dialog, /const previouslyFocusedRef = useRef<HTMLElement \| null>\(null\)/);
assert.match(dialog, /if \(previous\?\.isConnected\) previous\.focus\(\)/);
assert.match(dialog, /const requestCloseRef = useRef\(requestClose\)/);
assert.match(dialog, /requestCloseRef\.current = requestClose/);
assert.match(dialog, /const confirmCloseStateRef = useRef\(confirmClose\)/);
assert.match(dialog, /\}, \[dismissCloseConfirm, visible\]\);/);

// Unsaved-close confirmation owns its own focus boundary and restores the previous editor control on cancel.
assert.match(dialog, /const closeConfirmRef = useRef<HTMLDivElement>\(null\)/);
assert.match(dialog, /const restoreFocusRef = useRef<HTMLElement \| null>\(null\)/);
assert.match(dialog, /data-action="cancel"[\s\S]{0,160}onClick=\{dismissCloseConfirm\}/);
assert.match(dialog, /const focusRoot = confirmCloseStateRef\.current \? closeConfirmRef\.current : dialogRef\.current/);
assert.match(dialog, /aria-describedby="remote-document-close-confirm-message"/);

// Layout/theme contract: Close stays in the title bar; Save belongs to the toolbar's far-right compact tier.
const headerEnd = dialog.indexOf("</header>");
assert.ok(headerEnd > 0, "remote document dialog must have a header");
const headerSource = dialog.slice(0, headerEnd);
assert.doesNotMatch(headerSource, /t\("common\.save"\)/, "Save must not return to the title bar");
assert.match(dialog, /<div className=\{styles\.toolbarActions\}>[\s\S]*className=\{`\$\{styles\.saveButton\} liquid-glass-button liquid-primary-button`\}/);
assert.match(dialogStyles, /\.saveButton\s*\{[\s\S]{0,160}height:\s*var\(--select-height\)/);
assert.match(dialogStyles, /\.saveButton\s*\{[\s\S]{0,240}margin-left:\s*auto/);

// BOM is a canonical themed toggle, not a native checkbox rendered directly in the toolbar.
assert.match(dialog, /liquid-glass-toggle/);
assert.match(dialog, /aria-label="BOM"/);
assert.match(dialogStyles, /\.bomToggle\s*\{[\s\S]{0,160}height:\s*var\(--select-height\)/);

console.log("Remote document editor contract passed.");
