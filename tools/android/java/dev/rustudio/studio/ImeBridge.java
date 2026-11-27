package dev.rustudio.studio;

import android.app.Activity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.os.Handler;
import android.os.Looper;
import android.text.Editable;
import android.text.InputType;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.util.Log;
import android.view.KeyCharacterMap;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.ExtractedText;
import android.view.inputmethod.ExtractedTextRequest;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

/**
 * The soft keyboard's editor on Android.
 *
 * A NativeActivity has no view that offers the keyboard an InputConnection,
 * so the keyboard runs in the framework's fallback mode: single characters
 * arrive as key events, and everything else a keyboard does — a word it
 * composed and committed, a suggestion, a swiped word, its paste key — is a
 * KeyEvent.ACTION_MULTIPLE whose characters only the Java object carries;
 * native code never sees them (text "missing"), and keyboards that both
 * compose and send keys deliver some of it twice ("doubled", "garbled").
 *
 * This view is added to the activity's content (1 x 1 px, draws nothing,
 * takes no touches: the window's input queue is the native code's). While
 * the editor has a text field it takes the view focus, and the keyboard is
 * shown for it; its {@link Conn} is a BaseInputConnection over a
 * SpannableStringBuilder that mirrors the field (text, selection, the
 * composing span), so the keyboard works as it does in an EditText:
 * suggestions, auto-correction, swipe, the clipboard key, deleting whole
 * words. Every change the keyboard makes to the mirror is reported to the
 * editor as one replacement — characters [a, b) of the text become `text`,
 * the selection moves to [selStart, selEnd) — found by comparing the mirror
 * before and after the operation (or the batch of operations the keyboard
 * bracketed with begin/endBatchEdit). Positions are in Unicode code points,
 * what the editor's text fields count in. The editor's text is the truth:
 * when it changes by other means (a tap moved the caret, a paste, undo, the
 * focus moved to another field), the editor sends the mirror its state
 * ({@link #setState}), with the number of the last operation that state
 * includes, so a state that is behind the keyboard's latest operation is
 * left for the editor's next one.
 *
 * After each of its operations the keyboard is told the mirror's selection
 * (and the whole text, if it asked to follow it), as a TextView tells it
 * after every edit (InputMethodManager.updateSelection / updateExtractedText):
 * a keyboard keeps its own idea of where the caret is and composes and
 * deletes by it — one never told that its Enter had moved the caret to the
 * next line went on typing at the end of the one before. In a code field
 * the native side has its say on what was typed before the operation is
 * reported ({@link #nativeCodeFixup}: the editor's auto-indent, made on the
 * mirror inside the operation so the one replacement reported holds it).
 *
 * Built into classes.dex by tools/android/build-dex.sh; loaded by
 * src/android/ime.rs with an InMemoryDexClassLoader (no Java in the build),
 * which binds the native method below. Everything here runs on the Java
 * main thread (the entry points post to it); nothing waits on the editor's
 * thread.
 */
public final class ImeBridge extends View {
    private static final String TAG = "rustudio";

    /** What is typed into (ime.rs `Kind`). */
    public static final int KIND_TEXT = 0;       // one line: Properties, Explorer names, filters
    public static final int KIND_MULTILINE = 1;  // a multi-line TextBox
    public static final int KIND_CODE = 2;       // the script editor: no suggestions or capitals

    /** Operations reported to the editor (ime.rs `Op`). */
    static final int OP_REPLACE = 1;  // [a, b) becomes `text`; selection [selStart, selEnd)
    static final int OP_SELECT = 2;   // the selection alone moved to [selStart, selEnd)
    static final int OP_ENTER = 3;    // the keyboard's Enter / action key in a one-line field

    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    private static ImeBridge sView;

    private final SpannableStringBuilder mText = new SpannableStringBuilder();
    private int mKind = KIND_TEXT;
    /** The last operation's number (`setState` says which one it includes). */
    private int mSeq;
    private Conn mConn;
    /** The keyboard asked to follow the whole text (getExtractedText with a monitor flag): its token. */
    private boolean mExtractMonitor;
    private int mExtractToken;

    /**
     * An operation the keyboard made on the mirror (the editor applies it to
     * its field). `hash`: the mirror's text after it (FNV-1a over code points,
     * ime.rs `hash`), for the editor to check it ended up the same.
     * `composing`: the keyboard is composing (a word being typed with
     * suggestions): the selection is the keyboard's, not to be judged.
     */
    private static native void nativeOp(int seq, int op, String text, int a, int b, int selStart, int selEnd, boolean composing, long hash);

    /**
     * The editor's auto-indent (src/code_indent.rs, bound by ime.rs): the
     * keyboard inserted `inserted` into a code field whose text is now `text`
     * with the caret at `cursor` (code points); returns "a b cursor\ntext" —
     * code points [a, b) become text, the caret goes to cursor — or null.
     */
    private static native String nativeCodeFixup(String text, int cursor, String inserted);

    private ImeBridge(Context context) {
        super(context);
        // Focusable only while the editor wants the keyboard (`show` /
        // `hide`): a focused text editor in the window would have the
        // framework bring the keyboard up of its own accord, at start and
        // whenever the window regains the focus.
        setFocusable(false);
        setFocusableInTouchMode(false);
        setWillNotDraw(true);
        setImportantForAutofill(IMPORTANT_FOR_AUTOFILL_NO);
    }

    // ------------------------------------------------------------ the editor's entry points (any thread)

    /** Adds the view to the activity's content. Before anything else. */
    public static void attach(final Activity activity) {
        MAIN.post(new Runnable() {
            public void run() {
                if (sView != null) {
                    return;
                }
                ImeBridge v = new ImeBridge(activity);
                activity.addContentView(v, new ViewGroup.LayoutParams(1, 1));
                sView = v;
                Log.i(TAG, "ime: input connection view attached");
            }
        });
    }

    /**
     * A field of `kind` has the focus, with `text` and the selection
     * [selStart, selEnd) (code points): the mirror takes its state and the
     * keyboard is asked up for it. Also for the keyboard's return after the
     * user hid it (same field).
     */
    public static void show(final int kind, final String text, final int selStart, final int selEnd, final int seq) {
        MAIN.post(new Runnable() {
            public void run() {
                ImeBridge v = sView;
                if (v == null) {
                    return;
                }
                boolean restart = v.mKind != kind || v.hasComposing() || !v.sameText(text);
                v.mKind = kind;
                v.setText(text, selStart, selEnd);
                v.mSeq = seq;
                v.setFocusable(true);
                v.setFocusableInTouchMode(true);
                if (!v.hasFocus()) {
                    v.requestFocus();
                    // (a new focus starts the input anew by itself)
                    restart = false;
                }
                final InputMethodManager imm = v.imm();
                if (imm == null) {
                    Log.w(TAG, "ime: no InputMethodManager");
                    return;
                }
                if (restart) {
                    // (the keyboard's idea of the field is from another: told anew)
                    imm.restartInput(v);
                } else {
                    v.tellSelection(imm);
                }
                if (!imm.showSoftInput(v, 0)) {
                    // (the focus just moved: the manager catches up with it
                    // on its next turn)
                    final ImeBridge view = v;
                    MAIN.post(new Runnable() {
                        public void run() {
                            if (view.hasFocus() && !imm.showSoftInput(view, 0)) {
                                Log.w(TAG, "ime: showSoftInput refused (window focus " + view.hasWindowFocus() + ")");
                            }
                        }
                    });
                }
            }
        });
    }

    /** Nothing to type into: the keyboard is asked down. */
    public static void hide() {
        MAIN.post(new Runnable() {
            public void run() {
                ImeBridge v = sView;
                if (v == null) {
                    return;
                }
                InputMethodManager imm = v.imm();
                if (imm != null) {
                    imm.hideSoftInputFromWindow(v.getWindowToken(), 0);
                }
                // (the focus goes back to the activity's own view, which no
                // keyboard comes up for by itself)
                v.clearFocus();
                v.setFocusable(false);
                v.setFocusableInTouchMode(false);
            }
        });
    }

    /**
     * The field's state as the editor has it, after a change the keyboard
     * didn't make (or one that didn't come out as the keyboard's operation
     * said): the mirror takes it — unless the keyboard has made operations
     * since the one (`seq`) this state includes; the editor's next state
     * will say. `restart`: the keyboard is told anew (its composition dropped).
     */
    public static void setState(final String text, final int selStart, final int selEnd, final int seq, final boolean restart) {
        MAIN.post(new Runnable() {
            public void run() {
                ImeBridge v = sView;
                if (v == null || seq != v.mSeq) {
                    return;
                }
                boolean textChanged = !v.sameText(text);
                boolean composing = v.hasComposing();
                v.setText(text, selStart, selEnd);
                InputMethodManager imm = v.imm();
                if (imm == null) {
                    return;
                }
                if (restart || composing || (textChanged && v.mKind != KIND_CODE)) {
                    imm.restartInput(v);
                } else {
                    v.tellSelection(imm);
                }
            }
        });
    }

    /** The system clipboard's text, or null (empty, or not this app's to read). */
    public static String clipboardGet(Context context) {
        ClipboardManager cm = (ClipboardManager) context.getSystemService(Context.CLIPBOARD_SERVICE);
        if (cm == null) {
            return null;
        }
        ClipData clip = cm.getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) {
            return null;
        }
        CharSequence cs = clip.getItemAt(0).coerceToText(context);
        if (cs == null || cs.length() == 0) {
            return null;
        }
        return cs.toString();
    }

    /** Puts `text` on the system clipboard. */
    public static void clipboardSet(Context context, String text) {
        ClipboardManager cm = (ClipboardManager) context.getSystemService(Context.CLIPBOARD_SERVICE);
        if (cm != null) {
            cm.setPrimaryClip(ClipData.newPlainText("rustudio", text));
        }
    }

    // ------------------------------------------------------------ the mirror

    private InputMethodManager imm() {
        return (InputMethodManager) getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
    }

    private boolean sameText(String text) {
        return mText.length() == text.length() && mText.toString().equals(text);
    }

    private boolean hasComposing() {
        return BaseInputConnection.getComposingSpanStart(mText) >= 0;
    }

    /** The mirror becomes `text` with the selection (code points), nothing composing. */
    private void setText(String text, int selStart, int selEnd) {
        if (!sameText(text)) {
            mText.replace(0, mText.length(), text);
        }
        BaseInputConnection.removeComposingSpans(mText);
        int a = unitIndex(text, selStart), b = unitIndex(text, selEnd);
        Selection.setSelection(mText, a, b);
    }

    private void tellSelection(InputMethodManager imm) {
        int a = Selection.getSelectionStart(mText), b = Selection.getSelectionEnd(mText);
        int ca = BaseInputConnection.getComposingSpanStart(mText), cb = BaseInputConnection.getComposingSpanEnd(mText);
        if (mExtractMonitor) {
            imm.updateExtractedText(this, mExtractToken, extracted());
        }
        imm.updateSelection(this, a, b, ca, cb);
    }

    /** The mirror as an ExtractedText (the whole of it). */
    private ExtractedText extracted() {
        ExtractedText et = new ExtractedText();
        et.text = mText.toString();
        et.startOffset = 0;
        et.partialStartOffset = -1;
        et.partialEndOffset = -1;
        et.selectionStart = Math.max(0, Selection.getSelectionStart(mText));
        et.selectionEnd = Math.max(0, Selection.getSelectionEnd(mText));
        et.flags = mKind == KIND_TEXT ? ExtractedText.FLAG_SINGLE_LINE : 0;
        return et;
    }

    /** The UTF-16 index of code point `cp` of `s` (its length when past the end). */
    private static int unitIndex(String s, int cp) {
        if (cp <= 0) {
            return 0;
        }
        int n = s.length(), i = 0;
        while (cp > 0 && i < n) {
            i += Character.charCount(s.codePointAt(i));
            cp--;
        }
        return i;
    }

    /** ime.rs `hash`: FNV-1a, 64 bits, over the code points. */
    static long hash(CharSequence s) {
        long h = 0xcbf29ce484222325L;
        int n = s.length();
        for (int i = 0; i < n; ) {
            int cp = Character.codePointAt(s, i);
            h ^= cp;
            h *= 0x100000001b3L;
            i += Character.charCount(cp);
        }
        return h;
    }

    @Override
    public boolean onCheckIsTextEditor() {
        return true;
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo out) {
        int type = InputType.TYPE_CLASS_TEXT;
        int options = EditorInfo.IME_FLAG_NO_FULLSCREEN | EditorInfo.IME_FLAG_NO_EXTRACT_UI;
        switch (mKind) {
            case KIND_CODE:
                // No suggestions, capitals or corrections in code. Samsung's
                // keyboard ignores the flag alone and keeps correcting; the
                // visible-password variation is what turns it off there.
                type |= InputType.TYPE_TEXT_FLAG_MULTI_LINE | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS | InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD;
                options |= EditorInfo.IME_ACTION_NONE | EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING;
                break;
            case KIND_MULTILINE:
                type |= InputType.TYPE_TEXT_FLAG_MULTI_LINE | InputType.TYPE_TEXT_FLAG_AUTO_CORRECT;
                options |= EditorInfo.IME_ACTION_NONE;
                break;
            default:
                type |= InputType.TYPE_TEXT_FLAG_AUTO_CORRECT;
                options |= EditorInfo.IME_ACTION_DONE;
                break;
        }
        out.inputType = type;
        out.imeOptions = options;
        out.initialSelStart = Selection.getSelectionStart(mText);
        out.initialSelEnd = Selection.getSelectionEnd(mText);
        out.initialCapsMode = 0;
        out.packageName = getContext().getPackageName();
        out.fieldId = getId();
        mExtractMonitor = false;
        mConn = new Conn();
        return mConn;
    }

    /** The keyboard's operations on the mirror, each reported to the editor. */
    private final class Conn extends BaseInputConnection {
        private int mBatch;
        private String mBefore = "";
        private int mSelBeforeA, mSelBeforeB;

        Conn() {
            super(ImeBridge.this, true);
        }

        @Override
        public Editable getEditable() {
            return mText;
        }

        private void snapshot() {
            mBefore = mText.toString();
            mSelBeforeA = Selection.getSelectionStart(mText);
            mSelBeforeB = Selection.getSelectionEnd(mText);
        }

        /**
         * Runs one operation; outside a batch of the keyboard's, it is
         * reported by itself. The batch count is held up through the run:
         * BaseInputConnection's commitText, setComposingText,
         * setComposingRegion, finishComposingText and the two
         * deleteSurroundingText each wrap their work in beginBatchEdit /
         * endBatchEdit — this class's overrides — so an operation the
         * keyboard made outside a batch of its own was reported twice,
         * once as that inner batch ended and once here, the same
         * replacement both times: the editor typed every letter, paste and
         * deletion of such a keyboard twice (keyboards that batch their
         * calls, as Gboard does, were reported once).
         */
        private boolean mutate(Mut m) {
            boolean top = mBatch == 0;
            if (top) {
                snapshot();
            }
            mBatch++;
            boolean ok;
            try {
                ok = m.run();
            } finally {
                mBatch--;
            }
            if (top) {
                finish();
            }
            return ok;
        }

        /**
         * The end of an operation (or a batch): the editor's say on code
         * typed, the report to the editor, and the keyboard told where the
         * caret is now (the class comment).
         */
        private void finish() {
            if (mKind == KIND_CODE && !hasComposing()) {
                codeFixup();
            }
            report();
            InputMethodManager imm = imm();
            if (imm != null) {
                tellSelection(imm);
            }
        }

        /**
         * What changed since the snapshot: {p, s}, the common prefix and
         * suffix of the text before and after (UTF-16 units; a pair of
         * surrogates is replaced whole).
         */
        private int[] diff() {
            String now = mText.toString();
            String before = mBefore;
            int n0 = before.length(), n1 = now.length();
            int p = 0;
            while (p < n0 && p < n1 && before.charAt(p) == now.charAt(p)) {
                p++;
            }
            if (p > 0 && p < n0 && p < n1 && Character.isHighSurrogate(before.charAt(p - 1))) {
                p--;
            }
            int s = 0;
            while (s < n0 - p && s < n1 - p && before.charAt(n0 - 1 - s) == now.charAt(n1 - 1 - s)) {
                s++;
            }
            if (s > 0 && Character.isLowSurrogate(now.charAt(n1 - s)) && (n1 - s > p || n0 - s > p)) {
                s--;
            }
            return new int[] {p, s};
        }

        /**
         * A code field: text was inserted with the caret left after it (a
         * letter, a word, Enter) — the native side may indent (the method's
         * comment); its replacement is made on the mirror here, inside the
         * operation.
         */
        private void codeFixup() {
            String now = mText.toString();
            int n0 = mBefore.length(), n1 = now.length();
            int[] d = diff();
            int p = d[0], s = d[1];
            if (p + s >= n1) {
                return;
            }
            int selA = Selection.getSelectionStart(mText), selB = Selection.getSelectionEnd(mText);
            if (selA < 0 || selA != selB) {
                return;
            }
            String inserted = now.substring(p, n1 - s);
            int cursor = now.codePointCount(0, selA);
            if (cursor < inserted.codePointCount(0, inserted.length())) {
                return;
            }
            String fix;
            try {
                fix = nativeCodeFixup(now, cursor, inserted);
            } catch (UnsatisfiedLinkError e) {
                return;
            }
            if (fix == null) {
                return;
            }
            int nl = fix.indexOf('\n');
            if (nl < 0) {
                return;
            }
            String[] h = fix.substring(0, nl).split(" ");
            if (h.length != 3) {
                return;
            }
            try {
                int a = unitIndex(now, Integer.parseInt(h[0])), b = unitIndex(now, Integer.parseInt(h[1]));
                if (a > b || b > mText.length()) {
                    return;
                }
                mText.replace(a, b, fix.substring(nl + 1));
                int c = unitIndex(mText.toString(), Integer.parseInt(h[2]));
                Selection.setSelection(mText, c);
            } catch (NumberFormatException e) {
                Log.w(TAG, "ime: code fixup unreadable: " + fix.substring(0, nl));
            }
        }

        @Override
        public boolean beginBatchEdit() {
            if (mBatch++ == 0) {
                snapshot();
            }
            return true;
        }

        @Override
        public boolean endBatchEdit() {
            if (mBatch > 0 && --mBatch == 0) {
                finish();
            }
            return mBatch > 0;
        }

        /** What the operation(s) since the snapshot changed, as one replacement. */
        private void report() {
            String now = mText.toString();
            String before = mBefore;
            int n0 = before.length(), n1 = now.length();
            int[] d = diff();
            int p = d[0], s = d[1];
            int selA = Selection.getSelectionStart(mText), selB = Selection.getSelectionEnd(mText);
            if (selA < 0) {
                selA = n1;
            }
            if (selB < 0) {
                selB = n1;
            }
            int lo = Math.min(selA, selB), hi = Math.max(selA, selB);
            int selStart = now.codePointCount(0, lo), selEnd = selStart + now.codePointCount(lo, hi);
            boolean composing = getComposingSpanStart(mText) >= 0;
            if (p + s < n0 || p + s < n1) {
                int a = before.codePointCount(0, p);
                int b = a + before.codePointCount(p, n0 - s);
                nativeOp(++mSeq, OP_REPLACE, now.substring(p, n1 - s), a, b, selStart, selEnd, composing, hash(now));
            } else if (selA != mSelBeforeA || selB != mSelBeforeB) {
                nativeOp(++mSeq, OP_SELECT, "", 0, 0, selStart, selEnd, composing, hash(now));
            }
        }

        /** The Enter / action key: a line in a multi-line field, the field's end otherwise. */
        private void enter() {
            if (mKind == KIND_TEXT) {
                int n = mText.length();
                int selA = Selection.getSelectionStart(mText), selB = Selection.getSelectionEnd(mText);
                int lo = Math.min(selA < 0 ? n : selA, selB < 0 ? n : selB), hi = Math.max(selA < 0 ? n : selA, selB < 0 ? n : selB);
                int selStart = mText.toString().codePointCount(0, lo), selEnd = selStart + mText.toString().codePointCount(lo, hi);
                nativeOp(++mSeq, OP_ENTER, "", 0, 0, selStart, selEnd, false, hash(mText));
            } else {
                mutate(new Mut() {
                    public boolean run() {
                        return Conn.super.commitText("\n", 1);
                    }
                });
            }
        }

        @Override
        public boolean commitText(final CharSequence text, final int newCursorPosition) {
            if (text != null && text.length() == 1 && text.charAt(0) == '\n') {
                enter();
                return true;
            }
            return mutate(new Mut() {
                public boolean run() {
                    return Conn.super.commitText(text, newCursorPosition);
                }
            });
        }

        @Override
        public boolean setComposingText(final CharSequence text, final int newCursorPosition) {
            return mutate(new Mut() {
                public boolean run() {
                    return Conn.super.setComposingText(text, newCursorPosition);
                }
            });
        }

        @Override
        public boolean setComposingRegion(final int start, final int end) {
            return mutate(new Mut() {
                public boolean run() {
                    return Conn.super.setComposingRegion(start, end);
                }
            });
        }

        @Override
        public boolean finishComposingText() {
            return mutate(new Mut() {
                public boolean run() {
                    return Conn.super.finishComposingText();
                }
            });
        }

        @Override
        public boolean deleteSurroundingText(final int before, final int after) {
            return mutate(new Mut() {
                public boolean run() {
                    return Conn.super.deleteSurroundingText(before, after);
                }
            });
        }

        @Override
        public boolean deleteSurroundingTextInCodePoints(final int before, final int after) {
            return mutate(new Mut() {
                public boolean run() {
                    return Conn.super.deleteSurroundingTextInCodePoints(before, after);
                }
            });
        }

        @Override
        public boolean setSelection(final int start, final int end) {
            return mutate(new Mut() {
                public boolean run() {
                    return Conn.super.setSelection(start, end);
                }
            });
        }

        @Override
        public boolean performEditorAction(int action) {
            enter();
            return true;
        }

        @Override
        public boolean performContextMenuAction(int id) {
            // The keyboard's own Select all / Cut / Copy / Paste (some
            // keyboards' clipboard panels use these).
            if (id == android.R.id.selectAll) {
                return setSelection(0, mText.length());
            }
            if (id == android.R.id.paste) {
                String text = clipboardGet(getContext());
                return text != null && commitText(text, 1);
            }
            if (id == android.R.id.copy || id == android.R.id.cut) {
                int a = Selection.getSelectionStart(mText), b = Selection.getSelectionEnd(mText);
                if (a < 0 || b < 0 || a == b) {
                    return false;
                }
                clipboardSet(getContext(), mText.subSequence(Math.min(a, b), Math.max(a, b)).toString());
                return id == android.R.id.copy || commitText("", 1);
            }
            return false;
        }

        @Override
        public boolean sendKeyEvent(KeyEvent event) {
            // Enter as a key (some keyboards send it so for the action key):
            // as the Enter above, so a one-line field's edit ends and a
            // multi-line field gets its line once, not also as a key event
            // the native side would turn into a second one.
            if (event.getKeyCode() == KeyEvent.KEYCODE_ENTER && event.getAction() == KeyEvent.ACTION_DOWN) {
                enter();
                return true;
            }
            if (event.getKeyCode() == KeyEvent.KEYCODE_ENTER) {
                return true;
            }
            // Deleting by key (a keyboard without the newer calls): on the
            // mirror, so the editor is told what to delete; the key itself
            // would go through the window to the native side and delete once
            // more.
            if (event.getAction() == KeyEvent.ACTION_DOWN && event.getKeyCode() == KeyEvent.KEYCODE_DEL) {
                int a = Selection.getSelectionStart(mText), b = Selection.getSelectionEnd(mText);
                if (a >= 0 && b >= 0 && a != b) {
                    return commitText("", 1);
                }
                return deleteSurroundingTextInCodePoints(1, 0);
            }
            if (event.getAction() == KeyEvent.ACTION_DOWN && event.getKeyCode() == KeyEvent.KEYCODE_FORWARD_DEL) {
                return deleteSurroundingTextInCodePoints(0, 1);
            }
            if (event.getKeyCode() == KeyEvent.KEYCODE_DEL || event.getKeyCode() == KeyEvent.KEYCODE_FORWARD_DEL) {
                return true;
            }
            // A character sent as a key (a keyboard in its key-event mode, a
            // hardware keyboard through the IME): the character goes to the
            // mirror; the key event itself is kept from the window, which
            // would type it a second time.
            if (event.getAction() == KeyEvent.ACTION_DOWN) {
                int ch = event.getUnicodeChar(event.getMetaState());
                if (ch != 0 && (ch & KeyCharacterMap.COMBINING_ACCENT) == 0 && !Character.isISOControl(ch)) {
                    return commitText(new String(Character.toChars(ch)), 1);
                }
            } else if (event.getAction() == KeyEvent.ACTION_UP) {
                int ch = event.getUnicodeChar(event.getMetaState());
                if (ch != 0 && (ch & KeyCharacterMap.COMBINING_ACCENT) == 0 && !Character.isISOControl(ch)) {
                    return true;
                }
            }
            // Arrows, Escape, Tab, the rest: through the window to the native side.
            return super.sendKeyEvent(event);
        }

        @Override
        public ExtractedText getExtractedText(ExtractedTextRequest request, int flags) {
            // (keyboards that follow the whole text ask for it so, and with
            // the monitor flag are told of every change: `tellSelection`)
            if ((flags & InputConnection.GET_EXTRACTED_TEXT_MONITOR) != 0 && request != null) {
                mExtractMonitor = true;
                mExtractToken = request.token;
            }
            return extracted();
        }

        @Override
        public boolean requestCursorUpdates(int cursorUpdateMode) {
            return false;
        }
    }

    private interface Mut {
        boolean run();
    }
}
