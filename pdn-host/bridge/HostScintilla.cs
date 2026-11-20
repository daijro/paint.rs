// CodeLab embeds ScintillaNET, whose Scintilla control funnels every text and state operation through a native
// DirectMessage(sciPtr, msg, wParam, lParam) -> the exported Scintilla_DirectFunction of a LoadLibrary'd native
// Scintilla DLL, with sciPtr the document pointer from SendMessage(Handle, SCI_GETDIRECTPOINTER). There is no native
// Scintilla in the browser, and none is wanted off Windows or even on Windows (paint.rs draws the editor itself), so
// the plug-in rewriter routes that funnel here instead: Create() stands in for the SCI_GETDIRECTPOINTER (it mints a
// document id used as sciPtr) and Message() stands in for the direct function. This is the managed Scintilla the
// control talks to; paint.rs's editor widget is bound to the same document through the "code" property and
// DialogCanvas.EditControl (its text, selection and edits), and CodeLab reacts through the control's notifications.
//
// Scintilla addresses text by UTF-8 BYTE offset with a line-start index (not char indices), and CodeLab's position
// math (SelectionStart/End, Lines, GotoPosition) is built on that, so the document mirrors it exactly.
using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

namespace PaintDotNet.Bridge;

/// One Scintilla document: its UTF-8 text, line-start index, selection and modify state, all in byte offsets.
internal sealed class HostScintillaDocument
{
    private byte[] buf = Array.Empty<byte>();          // UTF-8 bytes; the live text is buf[0..Length]
    private readonly List<int> lineStart = new() { 0 };// byte offset of each line start; lineStart[0] is always 0
    private GCHandle pin;                               // keeps buf fixed while a GETTEXT pointer the plug-in reads is live

    /// The editor control showing this document, once the host has found it (ScintillaBox.Of): it is told of each
    /// change as Scintilla tells its own (SCN_MODIFIED).
    public System.Windows.Forms.Control? Owner { get; set; }

    public int Length { get; private set; }
    public int Anchor { get; set; }                     // selection byte offsets (Anchor..Current, either order)
    public int Current { get; set; }
    public bool ReadOnly { get; set; }
    public bool Modified { get; private set; }

    public int LineCount => lineStart.Count;
    public int SelStart => Math.Min(Anchor, Current);
    public int SelEnd => Math.Max(Anchor, Current);

    public string GetText() => Encoding.UTF8.GetString(buf, 0, Length);

    public void SetText(string s)
    {
        Replace(Encoding.UTF8.GetBytes(s ?? string.Empty));
        Anchor = Current = 0;
    }

    public void ClearAll() { if (!ReadOnly) { Replace(Array.Empty<byte>()); Anchor = Current = 0; } }

    public void Insert(int bytePos, string s)
    {
        if (ReadOnly) return;
        bytePos = Math.Clamp(bytePos, 0, Length);
        byte[] ins = Encoding.UTF8.GetBytes(s ?? string.Empty);
        if (ins.Length == 0) return;
        var next = new byte[Length + ins.Length];
        Array.Copy(buf, 0, next, 0, bytePos);
        Array.Copy(ins, 0, next, bytePos, ins.Length);
        Array.Copy(buf, bytePos, next, bytePos + ins.Length, Length - bytePos);
        Replace(next);
        // As Scintilla moves positions for an insertion: those after it, not one at it (SCI_INSERTTEXT keeps a caret
        // at the insertion point before the text; ADDTEXT and REPLACESEL then put it after).
        if (Anchor > bytePos) Anchor += ins.Length;
        if (Current > bytePos) Current += ins.Length;
    }

    public void Append(string s) => Insert(Length, s);

    public void Delete(int bytePos, int byteLen)
    {
        if (ReadOnly) return;
        bytePos = Math.Clamp(bytePos, 0, Length);
        byteLen = Math.Clamp(byteLen, 0, Length - bytePos);
        if (byteLen == 0) return;
        var next = new byte[Length - byteLen];
        Array.Copy(buf, 0, next, 0, bytePos);
        Array.Copy(buf, bytePos + byteLen, next, bytePos, Length - bytePos - byteLen);
        Replace(next);
        Anchor = AfterDelete(Anchor, bytePos, byteLen);
        Current = AfterDelete(Current, bytePos, byteLen);
    }

    public string GetTextRange(int bytePos, int byteLen)
    {
        bytePos = Math.Clamp(bytePos, 0, Length);
        byteLen = Math.Clamp(byteLen, 0, Length - bytePos);
        return Encoding.UTF8.GetString(buf, bytePos, byteLen);
    }

    public byte ByteAt(int bytePos) => bytePos >= 0 && bytePos < Length ? buf[bytePos] : (byte)0;

    /// The greatest line whose start is <= bytePos (0-based), as SCI_LINEFROMPOSITION.
    public int LineFromPosition(int bytePos)
    {
        bytePos = Math.Clamp(bytePos, 0, Length);
        int lo = 0, hi = lineStart.Count - 1, ans = 0;
        while (lo <= hi) { int mid = (lo + hi) >> 1; if (lineStart[mid] <= bytePos) { ans = mid; lo = mid + 1; } else hi = mid - 1; }
        return ans;
    }

    public int PositionFromLine(int line) => line <= 0 ? 0 : line >= lineStart.Count ? Length : lineStart[line];

    /// The byte position `chars` characters after (or before, when negative) `pos`, as SCI_POSITIONRELATIVE; 0 when
    /// that runs off either end.
    public int Relative(int pos, int chars)
    {
        int p = Math.Clamp(pos, 0, Length);
        for (; chars > 0; chars--)
        {
            if (p >= Length) return 0;
            p++;
            while (p < Length && (buf[p] & 0xC0) == 0x80) p++;
        }
        for (; chars < 0; chars++)
        {
            if (p <= 0) return 0;
            p--;
            while (p > 0 && (buf[p] & 0xC0) == 0x80) p--;
        }
        return p;
    }

    // The target (SCI_SETTARGET*: what SEARCHINTARGET looks in and REPLACETARGET replaces) and the search flags.
    public int TargetStart { get; set; }
    public int TargetEnd { get; set; }
    public int SearchFlags { get; set; }
    public int TabWidth { get; set; } = 8;
    public int IndentWidth { get; set; }    // 0: the tab width, as Scintilla's
    public bool UseTabs { get; set; } = true;

    private const int MATCHCASE = 0x4, WHOLEWORD = 0x2, WORDSTART = 0x00100000, REGEXP = 0x00200000;

    /// UTF-16 code units in bytes [start, end), as SCI_COUNTCODEUNITS (a 4-byte UTF-8 character is two).
    public int CodeUnits(int start, int end)
    {
        start = Math.Clamp(start, 0, Length);
        end = Math.Clamp(end, start, Length);
        int n = 0;
        for (int i = start; i < end; i++)
            if ((buf[i] & 0xC0) != 0x80) n += buf[i] >= 0xF0 ? 2 : 1;
        return n;
    }

    // Scintilla's default character classes: word (letters, digits, '_' and every non-ASCII byte), line ends, other
    // blanks and control characters, and punctuation.
    private const int Space = 0, NewLine = 1, WordChar = 2, Punctuation = 3;
    private static int Class(byte b) => b is (byte)'\r' or (byte)'\n' ? NewLine : b <= 0x20 ? Space
        : b >= 0x80 || b == '_' || char.IsAsciiLetterOrDigit((char)b) ? WordChar : Punctuation;

    /// SCI_WORDSTARTPOSITION / SCI_WORDENDPOSITION: from `pos`, back (or on) over the characters of one class, the
    /// word characters only when `onlyWord`, otherwise the class of the character next to `pos`.
    public int WordStart(int pos, bool onlyWord)
    {
        pos = Math.Clamp(pos, 0, Length);
        int cls = onlyWord || pos == 0 ? WordChar : Class(buf[pos - 1]);
        while (pos > 0 && Class(buf[pos - 1]) == cls) pos--;
        return pos;
    }

    public int WordEnd(int pos, bool onlyWord)
    {
        pos = Math.Clamp(pos, 0, Length);
        int cls = onlyWord || pos == Length ? WordChar : Class(buf[pos]);
        while (pos < Length && Class(buf[pos]) == cls) pos++;
        return pos;
    }

    private bool WordBoundary(int pos) => pos <= 0 || pos >= Length || Class(buf[pos - 1]) != WordChar || Class(buf[pos]) != WordChar;

    /// SCI_BRACEMATCH: the brace matching the one at `pos` ((), [], {}, <>), nesting counted; -1 when there is none.
    /// ponytail: every character is one style here (paint.rs colours the text), so braces inside strings and
    /// comments count too; Scintilla skips those through its lexer's styles.
    public int BraceMatch(int pos)
    {
        if (pos < 0 || pos >= Length) return -1;
        byte open = buf[pos];
        (byte other, int dir) = open switch
        {
            (byte)'(' => ((byte)')', 1), (byte)')' => ((byte)'(', -1), (byte)'[' => ((byte)']', 1), (byte)']' => ((byte)'[', -1),
            (byte)'{' => ((byte)'}', 1), (byte)'}' => ((byte)'{', -1), (byte)'<' => ((byte)'>', 1), (byte)'>' => ((byte)'<', -1),
            _ => ((byte)0, 0),
        };
        if (dir == 0) return -1;
        for (int p = pos + dir, depth = 1; p >= 0 && p < Length; p += dir)
        {
            if (buf[p] == open) depth++;
            else if (buf[p] == other && --depth == 0) return p;
        }
        return -1;
    }

    /// SCI_SEARCHINTARGET: the first match of `what` in the target (the last when the target runs backwards), which
    /// becomes the target; -1 when there is none. Case folds ASCII only.
    /// ponytail: REGEXP uses .NET's regular expressions, which read Scintilla's basic patterns the same way.
    public int Search(byte[] what)
    {
        bool back = TargetStart > TargetEnd;
        int lo = Math.Clamp(Math.Min(TargetStart, TargetEnd), 0, Length), hi = Math.Clamp(Math.Max(TargetStart, TargetEnd), 0, Length);
        if (what.Length == 0) return -1;
        if ((SearchFlags & REGEXP) != 0)
        {
            string range = Encoding.UTF8.GetString(buf, lo, hi - lo);
            var options = (SearchFlags & MATCHCASE) != 0 ? System.Text.RegularExpressions.RegexOptions.None
                                                         : System.Text.RegularExpressions.RegexOptions.IgnoreCase;
            var matches = System.Text.RegularExpressions.Regex.Matches(range, Encoding.UTF8.GetString(what), options);
            if (matches.Count == 0) return -1;
            var m = back ? matches[^1] : matches[0];
            TargetStart = lo + Encoding.UTF8.GetByteCount(range.AsSpan(0, m.Index));
            TargetEnd = TargetStart + Encoding.UTF8.GetByteCount(m.Value);
            return TargetStart;
        }
        bool matchCase = (SearchFlags & MATCHCASE) != 0;
        for (int p = back ? hi - what.Length : lo; back ? p >= lo : p + what.Length <= hi; p += back ? -1 : 1)
        {
            int i = 0;
            while (i < what.Length && (matchCase ? buf[p + i] == what[i] : char.ToLowerInvariant((char)buf[p + i]) == char.ToLowerInvariant((char)what[i]))) i++;
            if (i < what.Length) continue;
            if ((SearchFlags & WHOLEWORD) != 0 && !(WordBoundary(p) && WordBoundary(p + what.Length))) continue;
            if ((SearchFlags & WORDSTART) != 0 && !WordBoundary(p)) continue;
            TargetStart = p;
            TargetEnd = p + what.Length;
            return p;
        }
        return -1;
    }

    /// SCI_GETCOLUMN: the column of `pos` on its line, tabs to the next multiple of the tab width.
    public int Column(int pos)
    {
        pos = Math.Clamp(pos, 0, Length);
        int col = 0;
        for (int i = PositionFromLine(LineFromPosition(pos)); i < pos; i++)
        {
            if (buf[i] == '\t') col = (col / TabWidth + 1) * TabWidth;
            else if ((buf[i] & 0xC0) != 0x80) col++;
        }
        return col;
    }

    /// SCI_GETLINEINDENTPOSITION: the byte after `line`'s leading blanks.
    public int IndentPosition(int line)
    {
        int p = PositionFromLine(line);
        while (p < Length && (buf[p] == ' ' || buf[p] == '\t')) p++;
        return p;
    }

    public void SetSavePoint() => Modified = false;
    public void EmptyUndoBuffer() { }   // no undo stack in the stand-in; nothing to drop

    /// Pins the buffer and returns the native address of byte `offset`, for the plug-in's GETTEXT/GETRANGEPOINTER
    /// read. Valid until the next edit (which re-pins); the plug-in copies out immediately, as Scintilla's does.
    public IntPtr Pointer(int offset)
    {
        if (!pin.IsAllocated) pin = GCHandle.Alloc(buf, GCHandleType.Pinned);
        return pin.AddrOfPinnedObject() + Math.Clamp(offset, 0, Math.Max(0, buf.Length));
    }

    private void Replace(byte[] next)
    {
        if (pin.IsAllocated) pin.Free();
        buf = next;
        Length = next.Length;
        Reindex();
        Modified = true;
    }

    private void Reindex()
    {
        lineStart.Clear();
        lineStart.Add(0);
        for (int i = 0; i < Length; i++)
            if (buf[i] == (byte)'\n') lineStart.Add(i + 1);   // a line begins just after each '\n' (a trailing '\n' => a final empty line)
    }

    private static int AfterDelete(int p, int pos, int len) => p <= pos ? p : p >= pos + len ? p - len : pos;
}

/// The direct-function stand-in the rewriter routes Scintilla.DirectMessage to, and the SCI_GETDIRECTPOINTER
/// stand-in (Create). sciPtr is a document id, not a real pointer. Public because the rewritten plug-in IL calls it.
public static class HostScintilla
{
    // SCI_* message numbers CodeLab's CodeTextBox reaches (from ScintillaNET 5's DirectMessage call sites).
    private const int SETTEXT = 2181, GETRANGEPOINTER = 2643, GETLENGTH = 2006, GETTEXTLENGTH = 2183, ADDTEXT = 2001,
        LINELENGTH = 2350, GETCHARACTERPOINTER = 2520, POSITIONRELATIVE = 2670, GETALLLINESVISIBLE = 2236,
        APPENDTEXT = 2282, CLEARALL = 2004, INSERTTEXT = 2003, DELETERANGE = 2645, GETTEXTRANGE = 2162,
        GETCHARAT = 2007, GETCURRENTPOS = 2008, SETCURRENTPOS = 2141, GOTOPOS = 2025, SETSEL = 2160,
        GETSELTEXT = 2161, GETSELECTIONSTART = 2143, SETSELECTIONSTART = 2142, GETSELECTIONEND = 2145,
        SETSELECTIONEND = 2144, GETANCHOR = 2009, SETANCHOR = 2026, GETREADONLY = 2140, SETREADONLY = 2171,
        GETMODIFY = 2159, SETSAVEPOINT = 2014, EMPTYUNDOBUFFER = 2175, GETLINECOUNT = 2154,
        LINEFROMPOSITION = 2166, POSITIONFROMLINE = 2167, GETLINEENDPOSITION = 2136, SETCODEPAGE = 2037,
        // What an editor's own code reads back: code-unit counts, the target and searching, words, braces, the
        // selection(s), indentation, the code page.
        COUNTCODEUNITS = 2715, GETCODEPAGE = 2137, SETTARGETSTART = 2190, GETTARGETSTART = 2191, SETTARGETEND = 2192,
        GETTARGETEND = 2193, SETTARGETRANGE = 2686, TARGETFROMSELECTION = 2287, TARGETWHOLEDOCUMENT = 2690,
        GETTARGETTEXT = 2687, SEARCHINTARGET = 2197, SETSEARCHFLAGS = 2198, GETSEARCHFLAGS = 2199, REPLACETARGET = 2194,
        REPLACESEL = 2170, CLEAR = 2180, SELECTALL = 2013, GOTOLINE = 2024, SETEMPTYSELECTION = 2556, BRACEMATCH = 2353,
        WORDSTARTPOSITION = 2266, WORDENDPOSITION = 2267, ISRANGEWORD = 2691, GETCOLUMN = 2129, SETTABWIDTH = 2036,
        GETTABWIDTH = 2121, SETUSETABS = 2124, GETUSETABS = 2125, SETINDENT = 2122, GETINDENT = 2123,
        SETLINEINDENTATION = 2126, GETLINEINDENTATION = 2127, GETLINEINDENTPOSITION = 2128, GETSELECTIONS = 2570,
        CLEARSELECTIONS = 2571, SETSELECTION = 2572, ADDSELECTION = 2573, GETMAINSELECTION = 2575,
        SETSELECTIONNCARET = 2576, GETSELECTIONNCARET = 2577, SETSELECTIONNANCHOR = 2578, GETSELECTIONNANCHOR = 2579,
        SETSELECTIONNSTART = 2584, GETSELECTIONNSTART = 2585, SETSELECTIONNEND = 2586, GETSELECTIONNEND = 2587;

    private static readonly Dictionary<nint, HostScintillaDocument> docs = new();
    // Ids from an odd base, so a control field that holds one is its editor's document (ScintillaBox.Of), not a
    // small number that happens to match.
    private static nint nextId = 0x5C1D0001;

    /// A new document; its id stands in for the SCI direct pointer the control caches.
    public static nint Create()
    {
        nint id = nextId++;
        docs[id] = new HostScintillaDocument();
        return id;
    }

    internal static HostScintillaDocument? Doc(nint sciPtr) => sciPtr != 0 && docs.TryGetValue(sciPtr, out var d) ? d : null;
    internal static void Destroy(nint sciPtr) => docs.Remove(sciPtr);

    /// Stands in for ScintillaNET's Lexilla.CreateLexer (the native Lexilla export it binds a delegate to), which the
    /// rewriter routes here. paint.rs does the editor's syntax colouring, so there is no native lexer; a non-null handle
    /// is all SetLexerByName needs to proceed, and the SCI_SETILEXER that follows is a no-op in Message. The handle is
    /// never dereferenced, so a constant sentinel suffices.
    public static nint CreateLexer(string lexerName) => 1;

    /// The form the rewriter calls: ScintillaNET's `directFunction(sciPtr, msg, w, l)` leaves the delegate on the
    /// stack as the first argument, so take it (and ignore it) ahead of the real arguments.
    public static nint Message(object directFn, nint sciPtr, int msg, nint wParam, nint lParam)
        => Message(sciPtr, msg, wParam, lParam);

    /// Stands in for the native Scintilla_DirectFunction. Returns what the matching SCI message returns.
    public static unsafe nint Message(nint sciPtr, int msg, nint wParam, nint lParam)
    {
        if (!docs.TryGetValue(sciPtr, out HostScintillaDocument? d)) return 0;
        switch (msg)
        {
            case GETLENGTH:
            case GETTEXTLENGTH: return d.Length;
            case GETLINECOUNT: return d.LineCount;
            case GETMODIFY: return d.Modified ? 1 : 0;
            case GETREADONLY: return d.ReadOnly ? 1 : 0;
            case SETREADONLY: d.ReadOnly = wParam != 0; return 0;
            case SETSAVEPOINT: d.SetSavePoint(); return 0;
            case EMPTYUNDOBUFFER: d.EmptyUndoBuffer(); return 0;
            case SETCODEPAGE: return 0;   // always UTF-8 here
            case CLEARALL:
                Remove(d, 0, d.Length);
                d.Anchor = d.Current = 0;
                return 0;
            case GETSELECTIONSTART: return d.SelStart;
            case GETSELECTIONEND: return d.SelEnd;
            case GETCURRENTPOS:
            case GETANCHOR: return msg == GETANCHOR ? d.Anchor : d.Current;
            case SETSELECTIONSTART: d.Anchor = (int)wParam; d.Current = Math.Max(d.Current, (int)wParam); return 0;
            case SETSELECTIONEND: d.Current = (int)wParam; return 0;
            case SETCURRENTPOS: d.Current = (int)wParam; return 0;
            case SETANCHOR: d.Anchor = (int)wParam; return 0;
            case GOTOPOS: d.Anchor = d.Current = (int)wParam; return 0;
            case SETSEL: d.Anchor = (int)wParam; d.Current = (int)lParam; return 0;
            case LINEFROMPOSITION: return d.LineFromPosition((int)wParam);
            case POSITIONFROMLINE: return d.PositionFromLine((int)wParam);
            case GETLINEENDPOSITION:
            {
                int line = (int)wParam, start = d.PositionFromLine(line);
                int end = line + 1 < d.LineCount ? d.PositionFromLine(line + 1) : d.Length;
                while (end > start && (d.ByteAt(end - 1) == '\n' || d.ByteAt(end - 1) == '\r')) end--;
                return end;
            }
            case GETCHARAT: return d.ByteAt((int)wParam);
            case SETTEXT:
                // As Scintilla does it: the old text removed, then the new inserted, each told after it happens.
                if (d.ReadOnly) return 0;
                Remove(d, 0, d.Length);
                Insert(d, 0, ReadCStr(lParam));
                d.Anchor = d.Current = 0;
                return 0;
            case INSERTTEXT: Insert(d, wParam == -1 ? d.Current : (int)wParam, ReadCStr(lParam)); return 0;
            case ADDTEXT:
            {
                string text = ReadUtf8(lParam, (int)wParam);
                int at = d.Current;
                Insert(d, at, text);
                d.Anchor = d.Current = at + Encoding.UTF8.GetByteCount(text);   // the caret after the text
                return 0;
            }
            case APPENDTEXT: Insert(d, d.Length, ReadUtf8(lParam, (int)wParam)); return 0;
            case DELETERANGE: Remove(d, (int)wParam, (int)lParam); return 0;
            case LINELENGTH:
            {
                int line = (int)wParam;
                return line < 0 || line >= d.LineCount ? 0 : (line + 1 < d.LineCount ? d.PositionFromLine(line + 1) : d.Length) - d.PositionFromLine(line);
            }
            case POSITIONRELATIVE: return d.Relative((int)wParam, (int)lParam);
            case GETALLLINESVISIBLE: return 1;
            case GETCHARACTERPOINTER: return d.Pointer(0);
            case GETSELTEXT:
                // wParam 0 returns the byte length; otherwise lParam is a buffer to fill (NUL-terminated).
                return FillBuffer(lParam, Encoding.UTF8.GetBytes(d.GetTextRange(d.SelStart, d.SelEnd - d.SelStart)));
            case GETRANGEPOINTER:
                // ScintillaNET reads text through a direct pointer to byte wParam (its length lParam): return it.
                return d.Pointer((int)wParam);
            case GETTEXTRANGE:
            {
                // lParam -> Sci_TextRange { Sci_CharacterRange { nint cpMin, cpMax }; nint lpstrText }.
                var tr = (TextRange*)lParam;
                byte[] bytes = Encoding.UTF8.GetBytes(d.GetTextRange((int)tr->cpMin, (int)(tr->cpMax - tr->cpMin)));
                return FillBuffer(tr->lpstrText, bytes);
            }
            case COUNTCODEUNITS: return d.CodeUnits((int)wParam, (int)lParam);
            case GETCODEPAGE: return 65001;   // SC_CP_UTF8
            case SETTARGETSTART: d.TargetStart = Pos(d, wParam); return 0;
            case GETTARGETSTART: return d.TargetStart;
            case SETTARGETEND: d.TargetEnd = Pos(d, wParam); return 0;
            case GETTARGETEND: return d.TargetEnd;
            case SETTARGETRANGE: d.TargetStart = Pos(d, wParam); d.TargetEnd = Pos(d, lParam); return 0;
            case TARGETFROMSELECTION: d.TargetStart = d.SelStart; d.TargetEnd = d.SelEnd; return 0;
            case TARGETWHOLEDOCUMENT: d.TargetStart = 0; d.TargetEnd = d.Length; return 0;
            case GETTARGETTEXT:
            {
                int a = Math.Min(d.TargetStart, d.TargetEnd), b = Math.Max(d.TargetStart, d.TargetEnd);
                return FillBuffer(lParam, Encoding.UTF8.GetBytes(d.GetTextRange(a, b - a)));
            }
            case SETSEARCHFLAGS: d.SearchFlags = (int)wParam; return 0;
            case GETSEARCHFLAGS: return d.SearchFlags;
            case SEARCHINTARGET: return d.Search(Encoding.UTF8.GetBytes(ReadUtf8(lParam, (int)wParam)));
            case REPLACETARGET:
            {
                // wParam -1: lParam is NUL-terminated. The target becomes the replacement; its length is returned.
                string text = wParam == -1 ? ReadCStr(lParam) : ReadUtf8(lParam, (int)wParam);
                int a = Math.Min(d.TargetStart, d.TargetEnd), b = Math.Max(d.TargetStart, d.TargetEnd);
                if (d.ReadOnly) return 0;
                Remove(d, a, b - a);
                Insert(d, a, text);
                d.TargetStart = a;
                d.TargetEnd = a + Encoding.UTF8.GetByteCount(text);
                return d.TargetEnd - a;
            }
            case REPLACESEL:
            {
                int a = d.SelStart;
                string text = ReadCStr(lParam);
                Remove(d, a, d.SelEnd - a);
                Insert(d, a, text);
                d.Anchor = d.Current = a + Encoding.UTF8.GetByteCount(text);   // the caret after the text
                return 0;
            }
            case CLEAR:
                // The selection, or the character after the caret when there is none.
                if (d.SelEnd > d.SelStart) Remove(d, d.SelStart, d.SelEnd - d.SelStart);
                else Remove(d, d.Current, d.Relative(d.Current, 1) is var n && n > d.Current ? n - d.Current : 0);
                return 0;
            case SELECTALL: d.Anchor = 0; d.Current = d.Length; return 0;
            case GOTOLINE: d.Anchor = d.Current = d.PositionFromLine((int)wParam); return 0;
            case SETEMPTYSELECTION: d.Anchor = d.Current = Pos(d, wParam); return 0;
            case BRACEMATCH: return d.BraceMatch((int)wParam);
            case WORDSTARTPOSITION: return d.WordStart((int)wParam, lParam != 0);
            case WORDENDPOSITION: return d.WordEnd((int)wParam, lParam != 0);
            case ISRANGEWORD:
                return (int)wParam < (int)lParam && d.WordStart((int)wParam, true) == (int)wParam
                       && d.WordEnd((int)lParam, true) == (int)lParam ? 1 : 0;
            case GETCOLUMN: return d.Column((int)wParam);
            case SETTABWIDTH: d.TabWidth = Math.Max(1, (int)wParam); return 0;
            case GETTABWIDTH: return d.TabWidth;
            case SETUSETABS: d.UseTabs = wParam != 0; return 0;
            case GETUSETABS: return d.UseTabs ? 1 : 0;
            case SETINDENT: d.IndentWidth = Math.Max(0, (int)wParam); return 0;
            case GETINDENT: return d.IndentWidth;
            case GETLINEINDENTPOSITION: return d.IndentPosition((int)wParam);
            case GETLINEINDENTATION: return d.Column(d.IndentPosition((int)wParam));
            case SETLINEINDENTATION:
            {
                int line = (int)wParam, indent = Math.Max(0, (int)lParam);
                if (line < 0 || line >= d.LineCount || d.ReadOnly) return 0;
                string blanks = d.UseTabs ? new string('\t', indent / d.TabWidth) + new string(' ', indent % d.TabWidth) : new string(' ', indent);
                int start = d.PositionFromLine(line), end = d.IndentPosition(line);
                if (d.GetTextRange(start, end - start) == blanks) return 0;
                Remove(d, start, end - start);
                Insert(d, start, blanks);
                return 0;
            }
            // One selection only: selection 0 is the main one.
            case GETSELECTIONS: return 1;
            case GETMAINSELECTION: return 0;
            case CLEARSELECTIONS: d.Anchor = d.Current = 0; return 0;
            case SETSELECTION:
            case ADDSELECTION: d.Current = Pos(d, wParam); d.Anchor = Pos(d, lParam); return 0;
            case GETSELECTIONNCARET: return d.Current;
            case GETSELECTIONNANCHOR: return d.Anchor;
            case GETSELECTIONNSTART: return d.SelStart;
            case GETSELECTIONNEND: return d.SelEnd;
            case SETSELECTIONNCARET: d.Current = Pos(d, lParam); return 0;
            case SETSELECTIONNANCHOR: d.Anchor = Pos(d, lParam); return 0;
            case SETSELECTIONNSTART: if (d.Anchor <= d.Current) d.Anchor = Pos(d, lParam); else d.Current = Pos(d, lParam); return 0;
            case SETSELECTIONNEND: if (d.Anchor <= d.Current) d.Current = Pos(d, lParam); else d.Anchor = Pos(d, lParam); return 0;
            default: return 0;   // an unhandled SCI message: a no-op (styling/markers/folding are paint.rs's, ignored)
        }
    }

    // An insertion, told to the document's editor after it lands.
    private static void Insert(HostScintillaDocument d, int at, string s)
    {
        if (d.ReadOnly || string.IsNullOrEmpty(s)) return;
        at = Math.Clamp(at, 0, d.Length);
        d.Insert(at, s);
        ScintillaBox.Inserted(d, at, Encoding.UTF8.GetByteCount(s));
    }

    // A removal, told to the document's editor after it, with the text it removed.
    private static void Remove(HostScintillaDocument d, int at, int len)
    {
        at = Math.Clamp(at, 0, d.Length);
        len = Math.Clamp(len, 0, d.Length - at);
        if (len == 0 || d.ReadOnly) return;
        string gone = d.GetTextRange(at, len);
        d.Delete(at, len);
        ScintillaBox.Deleted(d, at, gone);
    }

    private static int Pos(HostScintillaDocument d, nint p) => (int)Math.Clamp(p, 0, d.Length);

    private static string ReadCStr(nint p) => p == 0 ? string.Empty : Marshal.PtrToStringUTF8(p) ?? string.Empty;

    private static unsafe string ReadUtf8(nint p, int len)
    {
        if (p == 0 || len <= 0) return string.Empty;
        return Encoding.UTF8.GetString((byte*)p, len);
    }

    /// Copies `bytes` and a NUL into the plug-in's buffer at `dst`, and returns the byte count written (sans NUL).
    private static unsafe nint FillBuffer(nint dst, byte[] bytes)
    {
        if (dst == 0) return bytes.Length;
        var p = (byte*)dst;
        for (int i = 0; i < bytes.Length; i++) p[i] = bytes[i];
        p[bytes.Length] = 0;
        return bytes.Length;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct TextRange { public nint cpMin, cpMax, lpstrText; }

    /// A self-check of the byte-offset document model (run from the host self-test). Throws on a wrong result.
    public static void SelfTest()
    {
        nint id = Create();
        var d = Doc(id)!;
        d.SetText("abc\ndéf\n");                 // 'é' is two UTF-8 bytes, so byte length 9, char length 8
        Expect(d.Length == 9, "len");
        Expect(d.LineCount == 3, "lines");       // "abc", "déf", "" (after the trailing \n)
        Expect(d.LineFromPosition(0) == 0 && d.LineFromPosition(4) == 1 && d.LineFromPosition(9) == 2, "lineFromPos");
        Expect(d.PositionFromLine(1) == 4 && d.PositionFromLine(2) == 9, "posFromLine");
        d.Anchor = d.Current = 4;
        d.Insert(4, "X");                         // before 'd' on line 2
        Expect(d.GetText() == "abc\nXdéf\n" && d.Current == 4, "insert (a caret at the insertion point stays)");
        d.Delete(0, 4);                           // drop "abc\n"
        Expect(d.GetText() == "Xdéf\n" && d.LineCount == 2, "delete");
        d.Anchor = 0; d.Current = 1;
        Expect(d.GetTextRange(d.SelStart, d.SelEnd - d.SelStart) == "X", "selRange");
        Expect(Message(id, GETTEXTLENGTH, 0, 0) == d.Length, "msg length");
        Message(id, SETREADONLY, 1, 0);
        d.Insert(0, "no"); Expect(d.GetText() == "Xdéf\n", "readonly blocks edit");
        Message(id, SETREADONLY, 0, 0);
        const string code = "if (a) { b[1]; }\n\U0001F600x";   // an emoji: four UTF-8 bytes, two UTF-16 units
        d.SetText(code);
        Expect(Message(id, COUNTCODEUNITS, 0, d.Length) == code.Length, "code units");
        Expect(d.BraceMatch(7) == 15 && d.BraceMatch(15) == 7 && d.BraceMatch(3) == 5 && d.BraceMatch(0) == -1, "braces");
        Expect(d.WordStart(5, true) == 4 && d.WordEnd(0, true) == 2 && d.WordStart(6, false) == 5, "words");
        d.TargetStart = 0; d.TargetEnd = d.Length; d.SearchFlags = 0;
        Expect(d.Search("B"u8.ToArray()) == 9 && d.TargetEnd == 10, "search");
        d.TargetStart = d.Length; d.TargetEnd = 0;
        Expect(d.Search("{"u8.ToArray()) == 7, "search backwards");
        d.TargetStart = 0; d.TargetEnd = d.Length; d.SearchFlags = 0x4;
        Expect(d.Search("B"u8.ToArray()) == -1, "match case");
        Destroy(id);
    }

    private static void Expect(bool ok, string what) { if (!ok) throw new Exception("HostScintilla.SelfTest failed: " + what); }
}

/// A Scintilla editor in a plug-in's window, as paint.rs shows it: a "code" setting whose value is the editor's text,
/// selection, read-only state and whether it changed since its save point (positions in UTF-16 code units, as .NET
/// and paint.rs count them), edited through paint.rs's code editor (DialogCanvas.EditControl). An editor is any
/// control holding a HostScintilla document id in a field (ScintillaNET keeps its direct pointer so). Each edit is
/// applied to the document and told to the control as Scintilla tells its own: SCN_MODIFIED for the text removed and
/// the text added, SCN_CHARADDED and the KeyPress for each typed character, SCN_UPDATEUI after, each through the
/// control's WndProc as the reflected WM_NOTIFY it handles.
internal static class ScintillaBox
{
    private const int WM_REFLECT_NOTIFY = 0x2000 + 0x4E;
    private const uint SCN_CHARADDED = 2001, SCN_UPDATEUI = 2007, SCN_MODIFIED = 2008;
    private const int SC_MOD_INSERTTEXT = 0x1, SC_MOD_DELETETEXT = 0x2, SC_PERFORMED_USER = 0x10;
    private const int SC_UPDATE_CONTENT = 0x1, SC_UPDATE_SELECTION = 0x2;

    private const System.Reflection.BindingFlags Fields =
        System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.NonPublic
        | System.Reflection.BindingFlags.DeclaredOnly;

    /// The document `c` drives, or null when it is no Scintilla editor (or has not reached its document yet). The
    /// first time, `c` becomes the document's owner: its editor has followed none of the changes so far (it tracks
    /// its lines from the notifications alone, from an empty document), so it is told the whole text was inserted.
    internal static HostScintillaDocument? Of(System.Windows.Forms.Control c)
    {
        for (Type? t = c.GetType(); t is not null && t != typeof(System.Windows.Forms.Control); t = t.BaseType)
            foreach (System.Reflection.FieldInfo f in t.GetFields(Fields))
                if (f.FieldType == typeof(nint) && f.GetValue(c) is nint id && HostScintilla.Doc(id) is { } doc)
                {
                    if (doc.Owner is null)
                    {
                        doc.Owner = c;
                        Inserted(doc, 0, doc.Length);
                    }
                    return doc;
                }
        return null;
    }

    /// `bytes` bytes inserted at `at`, told to the document's editor (SCN_MODIFIED, SC_MOD_INSERTTEXT).
    internal static void Inserted(HostScintillaDocument doc, int at, int bytes)
    {
        if (doc.Owner is not { } c || bytes <= 0) return;
        string text = doc.GetTextRange(at, bytes);
        Modified(c, SC_MOD_INSERTTEXT, at, Encoding.UTF8.GetBytes(text), Lines(text));
    }

    /// `text` removed from `at`, told to the document's editor (SCN_MODIFIED, SC_MOD_DELETETEXT).
    internal static void Deleted(HostScintillaDocument doc, int at, string text)
    {
        if (doc.Owner is not { } c || text.Length == 0) return;
        Modified(c, SC_MOD_DELETETEXT, at, Encoding.UTF8.GetBytes(text), -Lines(text));
    }

    /// The "code" setting of the editor `c`, or null when `c` is none.
    internal static DialogSettings.Setting? Setting(System.Windows.Forms.Control c, string key, string label)
    {
        if (Of(c) is not { } now) return null;
        return new DialogSettings.Setting
        {
            // Its value as the window shows it now (Text, as JSON): the schema is written after its window is gone.
            Key = key, Type = "code", Label = label, Control = c, Text = Value(now),
            // A fresh window takes the text the user left (its selection too), as if typed over what it loaded.
            Apply = v =>
            {
                if (Of(c) is { } doc && v.ValueKind == JsonValueKind.Object && v.TryGetProperty("text", out JsonElement t))
                {
                    Replace(c, doc, t.GetString() ?? "", Selection(v));
                    Notify(c, new Notification { code = SCN_UPDATEUI, updated = SC_UPDATE_CONTENT | SC_UPDATE_SELECTION });
                }
            },
            Same = v => v.ValueKind == JsonValueKind.Object && v.TryGetProperty("text", out JsonElement t)
                        && Of(c) is { } doc && Shown(doc.GetText()) == t.GetString(),
        };
    }

    /// The setting's size and its value (WriteProperties' "default").
    internal static void Write(Utf8JsonWriter jw, DialogSettings.Setting s)
    {
        jw.WriteNumber("width", s.Control?.Width ?? 0);
        jw.WriteNumber("height", s.Control?.Height ?? 0);
        jw.WritePropertyName("default");
        jw.WriteRawValue(s.Text);
    }

    // The editor's value: {"text", "sel": [start, end], "readOnly", "dirty", "errors": []}.
    private static string Value(HostScintillaDocument doc)
    {
        string text = doc.GetText();
        using var stream = new System.IO.MemoryStream();
        using (var jw = new Utf8JsonWriter(stream))
        {
            jw.WriteStartObject();
            jw.WriteString("text", Shown(text));
            jw.WriteStartArray("sel");
            jw.WriteNumberValue(ToShown(text, Utf16(text, doc.SelStart)));
            jw.WriteNumberValue(ToShown(text, Utf16(text, doc.SelEnd)));
            jw.WriteEndArray();
            jw.WriteBoolean("readOnly", doc.ReadOnly);
            jw.WriteBoolean("dirty", doc.Modified);
            jw.WriteStartArray("errors");
            jw.WriteEndArray();
            jw.WriteEndObject();
        }
        return Encoding.UTF8.GetString(stream.ToArray());
    }

    /// Applies paint.rs's editor events to the editor `c` (DialogCanvas.EditControl): {"e": "text", "text", "sel"}
    /// the whole text after an edit, {"e": "char", "c"} each character typed, {"e": "sel", "sel"} a moved selection.
    /// False when `c` is no editor, so the events go to it as a canvas's.
    internal static bool Edit(System.Windows.Forms.Control c, JsonElement events)
    {
        if (Of(c) is not { } doc || events.ValueKind != JsonValueKind.Array) return false;
        JsonElement[] list = [.. events.EnumerateArray()];
        for (int i = 0; i < list.Length; i++)
        {
            JsonElement ev = list[i];
            switch (Kind(ev))
            {
                case "text" when ev.TryGetProperty("text", out JsonElement t):
                {
                    // The characters typed with this change (the "char" events after it) reach the window first, as
                    // Windows Forms raises KeyPress before the control takes the key; a key the window handles (an
                    // editor stepping over a closing bracket it put in itself) keeps the change out, and paint.rs takes
                    // the window's text back with its settings. CharAdded follows the change, as Scintilla raises it.
                    // ponytail: a handled key drops the whole change, with any other key typed in the same frame.
                    int typedEnd = i + 1;
                    while (typedEnd < list.Length && Kind(list[typedEnd]) == "char") typedEnd++;
                    bool handled = false;
                    for (int k = i + 1; k < typedEnd; k++) handled |= KeyPress(c, list[k]);
                    if (!handled)
                    {
                        Replace(c, doc, t.GetString() ?? "", Selection(ev));
                        for (int k = i + 1; k < typedEnd; k++) CharAdded(c, list[k]);
                    }
                    // UpdateUI last, after CharAdded, as Scintilla raises it once the change is drawn (an editor's
                    // CharAdded can still undo what it put in, such as a closing bracket stepped over).
                    Notify(c, new Notification { code = SCN_UPDATEUI, updated = SC_UPDATE_CONTENT | SC_UPDATE_SELECTION });
                    i = typedEnd - 1;
                    break;
                }
                case "char":   // a key typed with no change to the text
                    if (!KeyPress(c, ev)) CharAdded(c, ev);
                    break;
                case "sel" when Selection(ev) is { } sel:
                    string now = doc.GetText();
                    Select(doc, now, (FromShown(now, Shown(now), sel.Item1), FromShown(now, Shown(now), sel.Item2)));
                    Notify(c, new Notification { code = SCN_UPDATEUI, updated = SC_UPDATE_SELECTION });
                    break;
            }
        }
        return true;
    }

    private static string Kind(JsonElement ev) => ev.TryGetProperty("e", out JsonElement e) ? e.GetString() ?? "" : "";

    // KeyPress for each character of a "char" event (Enter is '\r' to it, as Windows Forms raises it); true when the
    // window handled one.
    private static bool KeyPress(System.Windows.Forms.Control c, JsonElement ev)
    {
        bool handled = false;
        if (ev.TryGetProperty("c", out JsonElement ch) && ch.GetString() is { Length: > 0 } typed)
            foreach (char u in typed)
            {
                var args = new System.Windows.Forms.KeyPressEventArgs(u == '\n' ? '\r' : u);
                Invoke(c, "OnKeyPress", args);
                handled |= args.Handled;
            }
        return handled;
    }

    private static void CharAdded(System.Windows.Forms.Control c, JsonElement ev)
    {
        if (ev.TryGetProperty("c", out JsonElement ch) && ch.GetString() is { Length: > 0 } typed)
            Notify(c, new Notification { code = SCN_CHARADDED, ch = char.ConvertToUtf32(typed, 0), characterSource = 0 });
    }

    // The text `next` in place of the document's: one removal and one insertion around what the two share at their
    // ends, each told to the control, then the selection.
    private static void Replace(System.Windows.Forms.Control c, HostScintillaDocument doc, string shown, (int, int)? shownSel)
    {
        string now = doc.GetText();
        if (doc.ReadOnly) return;
        // paint.rs edits with LF line ends: the document keeps its own (CRLF, Scintilla's default on Windows).
        bool crlf = Crlf(now);
        string next = crlf ? shown.Replace("\r\n", "\n").Replace("\n", "\r\n") : shown;
        (int, int)? sel = shownSel is { } ss ? (FromShown(next, shown, ss.Item1), FromShown(next, shown, ss.Item2)) : null;
        int head = 0, max = Math.Min(now.Length, next.Length);
        while (head < max && now[head] == next[head]) head++;
        int tail = 0;
        while (tail < max - head && now[now.Length - 1 - tail] == next[next.Length - 1 - tail]) tail++;
        int at = Encoding.UTF8.GetByteCount(now.AsSpan(0, head));
        string removed = now[head..(now.Length - tail)], added = next[head..(next.Length - tail)];
        if (removed.Length > 0)
        {
            doc.Delete(at, Encoding.UTF8.GetByteCount(removed));
            Deleted(doc, at, removed);
        }
        if (added.Length > 0)
        {
            doc.Insert(at, added);
            Inserted(doc, at, Encoding.UTF8.GetByteCount(added));
        }
        if (sel is { } s) Select(doc, next, s);
    }

    private static void Modified(System.Windows.Forms.Control c, int what, int at, byte[] text, int linesAdded)
    {
        GCHandle pin = GCHandle.Alloc(text, GCHandleType.Pinned);
        try
        {
            Notify(c, new Notification
            {
                code = SCN_MODIFIED, modificationType = what | SC_PERFORMED_USER, position = at,
                text = pin.AddrOfPinnedObject(), length = text.Length, linesAdded = linesAdded,
            });
        }
        finally { pin.Free(); }
    }

    private static void Select(HostScintillaDocument doc, string text, (int Start, int End) sel)
    {
        int Byte(int u16) => Encoding.UTF8.GetByteCount(text.AsSpan(0, Math.Clamp(u16, 0, text.Length)));
        (doc.Anchor, doc.Current) = (Byte(sel.Start), Byte(sel.End));
    }

    private static (int, int)? Selection(JsonElement v)
        => v.TryGetProperty("sel", out JsonElement s) && s.ValueKind == JsonValueKind.Array && s.GetArrayLength() == 2
            ? (s[0].GetInt32(), s[1].GetInt32()) : null;

    // paint.rs's code editor edits with LF line ends; a document with CRLF ones is shown through LF, positions mapped.
    private static bool Crlf(string text) => text.Contains("\r\n");
    private static string Shown(string text) => Crlf(text) ? text.Replace("\r\n", "\n") : text;

    // UTF-16 position `u16` of `text` as a position in its shown form: less each CR of a CRLF before it.
    private static int ToShown(string text, int u16)
    {
        if (!Crlf(text)) return u16;
        int crs = 0;
        for (int i = 0; i + 1 < text.Length && i < u16; i++)
            if (text[i] == '\r' && text[i + 1] == '\n') crs++;
        return u16 - crs;
    }

    // Position `pos` of `shown` (the LF form of `text`) as a position in `text`: plus a CR for each line end before it.
    private static int FromShown(string text, string shown, int pos)
    {
        if (!Crlf(text)) return pos;
        pos = Math.Clamp(pos, 0, shown.Length);
        int lfs = 0;
        for (int i = 0; i < pos; i++)
            if (shown[i] == '\n') lfs++;
        return pos + lfs;
    }

    // The UTF-16 position of byte `b` of `text`'s UTF-8 form.
    private static int Utf16(string text, int b)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(text);
        return Encoding.UTF8.GetCharCount(bytes, 0, Math.Clamp(b, 0, bytes.Length));
    }

    private static int Lines(string s) => s.Count(ch => ch == '\n');

    // Scintilla's SCNotification (its header, then the fields after it), laid out as the C struct is on this platform.
    [StructLayout(LayoutKind.Sequential)]
    private struct Notification
    {
        public nint hwndFrom, idFrom;
        public uint code;
        public nint position;
        public int ch, modifiers, modificationType;
        public nint text, length, linesAdded;
        public int message;
        public nint wParam, lParam, line;
        public int foldLevelNow, foldLevelPrev, margin, listType, x, y, token;
        public nint annotationLinesAdded;
        public int updated, listCompletionMethod, characterSource;
    }

    // The notification as the control's parent reflects it back to it (WM_NOTIFY), into its WndProc.
    private static void Notify(System.Windows.Forms.Control c, Notification n)
    {
        nint p = Marshal.AllocHGlobal(Marshal.SizeOf<Notification>());
        try
        {
            Marshal.StructureToPtr(n, p, false);
            var m = System.Windows.Forms.Message.Create(IntPtr.Zero, WM_REFLECT_NOTIFY, IntPtr.Zero, p);
            Invoke(c, "WndProc", m);
        }
        finally { Marshal.FreeHGlobal(p); }
    }

    private static void Invoke(System.Windows.Forms.Control c, string name, object arg)
    {
        Type parameter = arg is System.Windows.Forms.Message ? typeof(System.Windows.Forms.Message).MakeByRefType() : arg.GetType();
        System.Reflection.MethodInfo? m = typeof(System.Windows.Forms.Control).GetMethod(name,
            System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.NonPublic, [parameter]);
        try { m?.Invoke(c, [arg]); }
        catch (System.Reflection.TargetInvocationException ex) when (ex.InnerException is not null)
        {
            if (PluginHost.Trace) Console.Error.WriteLine($"script box {name}: {ex.InnerException}");
        }
    }
}
