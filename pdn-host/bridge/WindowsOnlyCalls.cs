// Calls a plug-in makes that only Windows accepts, taken out of its IL before it loads. Thread.SetApartmentState
// asks for a COM apartment (plug-ins read the clipboard on an STA thread, as Windows Forms wants); off Windows
// there are no apartments and .NET throws instead. Without the call the thread runs as it would anyway, and the
// clipboard is the host's (Windows Forms' Clipboard), which needs none.
// In the browser two more go: OutputDebugString (kernel32) hands a message to an attached debugger, and there is
// no kernel32 to call; SystemEvents tells a listener when the user changes colours, fonts or the display, .NET
// throws there as a listener is added, and nothing changes. (toe_head2001's OptionBasedLibrary makes both.) The
// desktop has stand-ins for both instead (native/user32.c, Win32SystemEvents), so a plug-in's library that makes
// them still loads from its file and finds the files beside it.
using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.Linq;
using System.Reflection;
using System.Reflection.Emit;
using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;

namespace PaintDotNet.Bridge;

internal static class WindowsOnlyCalls
{
    /// `image` with each call to Thread.SetApartmentState (and in the browser, OutputDebugString or a SystemEvents
    /// event's add or remove) made a no-op (its arguments popped, the rest of the five bytes nops), or null when it
    /// makes none.
    public static byte[]? Patch(byte[] image)
    {
        using var pe = new PEReader(ImmutableArray.Create(image));
        if (!pe.HasMetadata) return null;
        MetadataReader md = pe.GetMetadataReader();
        var targets = new Dictionary<int, int>();   // the method's token: how many values the call takes
        foreach (MemberReferenceHandle h in md.MemberReferences)
        {
            MemberReference m = md.GetMemberReference(h);
            if (m.Parent.Kind != HandleKind.TypeReference) continue;
            TypeReference t = md.GetTypeReference((TypeReferenceHandle)m.Parent);
            string name = md.GetString(m.Name), type = md.GetString(t.Namespace) + "." + md.GetString(t.Name);
            if (name == "SetApartmentState" && type == "System.Threading.Thread") targets[MetadataTokens.GetToken(h)] = 2;
            else if (OperatingSystem.IsBrowser() && type == "Microsoft.Win32.SystemEvents" && (name.StartsWith("add_", StringComparison.Ordinal) || name.StartsWith("remove_", StringComparison.Ordinal)))
                targets[MetadataTokens.GetToken(h)] = 1;
        }
        if (OperatingSystem.IsBrowser())
            foreach (MethodDefinitionHandle h in md.MethodDefinitions)
            {
                MethodImport import = md.GetMethodDefinition(h).GetImport();
                if (import.Module.IsNil || !md.GetString(md.GetModuleReference(import.Module).Name).StartsWith("kernel32", StringComparison.OrdinalIgnoreCase)) continue;
                if (md.GetString(import.Name) is "OutputDebugString" or "OutputDebugStringA" or "OutputDebugStringW") targets[MetadataTokens.GetToken(h)] = 1;
            }
        if (targets.Count == 0) return null;

        byte[] patched = (byte[])image.Clone();
        bool any = false;
        foreach (MethodDefinitionHandle mh in md.MethodDefinitions)
        {
            int rva = md.GetMethodDefinition(mh).RelativeVirtualAddress;
            if (rva == 0 || Offset(pe, rva) is not int at) continue;
            // A tiny header is one byte; a fat one says its size, in 4-byte units, in its top flag bits.
            bool tiny = (image[at] & 3) == 2;
            int il = at + (tiny ? 1 : (BitConverter.ToUInt16(image, at) >> 12) * 4);
            int size = tiny ? image[at] >> 2 : BitConverter.ToInt32(image, at + 4);
            for (int i = il; i < il + size;)
            {
                OpCode op = image[i] == 0xFE ? Two[image[i + 1]] : One[image[i]];
                int start = i;
                i += op.Size + Operand(op, image, i + op.Size);
                if ((op == OpCodes.Call || op == OpCodes.Callvirt) && targets.TryGetValue(BitConverter.ToInt32(image, start + 1), out int pops))
                {
                    patched.AsSpan(start, pops).Fill(0x26);                 // pop each argument
                    patched.AsSpan(start + pops, 5 - pops).Clear();         // nops
                    any = true;
                }
            }
        }
        return any ? patched : null;
    }

    private static int? Offset(PEReader pe, int rva)
    {
        foreach (SectionHeader s in pe.PEHeaders.SectionHeaders)
            if (rva >= s.VirtualAddress && rva < s.VirtualAddress + Math.Max(s.VirtualSize, s.SizeOfRawData))
                return rva - s.VirtualAddress + s.PointerToRawData;
        return null;
    }

    private static int Operand(OpCode op, byte[] il, int at) => op.OperandType switch
    {
        OperandType.InlineNone => 0,
        OperandType.ShortInlineBrTarget or OperandType.ShortInlineI or OperandType.ShortInlineVar => 1,
        OperandType.InlineVar => 2,
        OperandType.InlineI8 or OperandType.InlineR => 8,
        OperandType.InlineSwitch => 4 + 4 * BitConverter.ToInt32(il, at),
        _ => 4,
    };

    private static readonly OpCode[] One = new OpCode[256], Two = new OpCode[256];
    static WindowsOnlyCalls()
    {
        foreach (FieldInfo f in typeof(OpCodes).GetFields(BindingFlags.Public | BindingFlags.Static))
        {
            var op = (OpCode)f.GetValue(null)!;
            if (op.Size == 1) One[(byte)op.Value] = op;
            else Two[(byte)op.Value] = op;
        }
    }
}
