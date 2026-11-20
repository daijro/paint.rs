// Checks the browser host's managed AES (app/Crypto.cs) against this platform's own: the FIPS 197 example vectors,
// then every mode, padding and key size over a spread of lengths, one-shot and through a CryptoStream in 7-byte
// writes, each way round (ours encrypts and the platform decrypts, and the reverse). Run it with dotnet run on a
// desktop, where .NET has AES; it prints each mismatch and exits non-zero if there is one.
using System;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using PaintDotNet.Bridge;

int failures = 0, cases = 0;
void Check(bool ok, string what) { cases++; if (!ok) { failures++; Console.WriteLine("FAIL " + what); } }
static byte[] Hex(string s) => Convert.FromHexString(s);
static byte[]? Try(Func<byte[]> f) { try { return f(); } catch (CryptographicException) { return null; } }
static byte[] Stream(ICryptoTransform t, byte[] data)
{
    var output = new MemoryStream();
    using (var cs = new CryptoStream(output, t, CryptoStreamMode.Write, leaveOpen: true))
        for (int at = 0; at < data.Length; at += 7) cs.Write(data, at, Math.Min(7, data.Length - at));
    return output.ToArray();
}
// What a decryption gives back is the data, plus the zeros of zero padding, which does not come off.
static bool Same(byte[] got, byte[] data, PaddingMode pad) =>
    pad == PaddingMode.Zeros ? got.Length >= data.Length && got.AsSpan(0, data.Length).SequenceEqual(data) && got.Skip(data.Length).All(b => b == 0)
                             : got.SequenceEqual(data);

// FIPS 197 appendix C.
byte[] pt = Hex("00112233445566778899aabbccddeeff");
foreach (var (key, ct) in new[] {
    ("000102030405060708090a0b0c0d0e0f", "69c4e0d86a7b0430d8cdb78070b4c55a"),
    ("000102030405060708090a0b0c0d0e0f1011121314151617", "dda97ca4864cdfe06eaf70a0ec0d7191"),
    ("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f", "8ea2b7ca516745bfeafc49904b496089") })
{
    using Aes a = PluginCrypto.CreateAes();
    a.Mode = CipherMode.ECB;
    a.Padding = PaddingMode.None;
    Check(a.CreateEncryptor(Hex(key), null).TransformFinalBlock(pt, 0, 16).SequenceEqual(Hex(ct)), $"FIPS 197 encrypt, {key.Length * 4}-bit key");
    Check(a.CreateDecryptor(Hex(key), null).TransformFinalBlock(Hex(ct), 0, 16).SequenceEqual(pt), $"FIPS 197 decrypt, {key.Length * 4}-bit key");
}

var rng = new Random(1);
int[] lengths = [.. Enumerable.Range(0, 49), 255, 1000, 4099];
foreach (int keyBits in new[] { 128, 192, 256 })
foreach (var (mode, feedback) in new[] { (CipherMode.ECB, 128), (CipherMode.CBC, 128), (CipherMode.CFB, 8), (CipherMode.CFB, 128) })
foreach (PaddingMode pad in Enum.GetValues<PaddingMode>())
foreach (int n in lengths)
{
    byte[] key = new byte[keyBits / 8], iv = new byte[16], data = new byte[n];
    rng.NextBytes(key);
    rng.NextBytes(iv);
    rng.NextBytes(data);
    Aes Set(Aes a) { a.KeySize = keyBits; a.Mode = mode; a.FeedbackSize = feedback; a.Padding = pad; a.Key = key; a.IV = iv; return a; }
    using Aes ours = Set(PluginCrypto.CreateAes()), real = Set(Aes.Create());
    string what = $"{keyBits}-bit {mode}{(mode == CipherMode.CFB ? feedback.ToString() : "")} {pad} {n} bytes";
    byte[]? theirs = Try(() => real.CreateEncryptor().TransformFinalBlock(data, 0, n));
    byte[]? mine = Try(() => ours.CreateEncryptor().TransformFinalBlock(data, 0, n));
    Check(theirs is null == mine is null, what + ": refuses the same inputs");
    if (theirs is null || mine is null) continue;
    if (pad != PaddingMode.ISO10126) Check(mine.SequenceEqual(theirs), what + ": same ciphertext");
    Check(Try(() => real.CreateDecryptor().TransformFinalBlock(mine, 0, mine.Length)) is { } back && Same(back, data, pad), what + ": the platform decrypts ours");
    Check(Try(() => ours.CreateDecryptor().TransformFinalBlock(theirs, 0, theirs.Length)) is { } d && Same(d, data, pad), what + ": ours decrypts the platform's");
    Check(Same(Stream(ours.CreateDecryptor(), theirs), data, pad), what + ": ours decrypts through a stream");
    byte[] streamed = Stream(ours.CreateEncryptor(), data);
    Check(pad == PaddingMode.ISO10126 ? Same(real.CreateDecryptor().TransformFinalBlock(streamed, 0, streamed.Length), data, pad)
                                      : streamed.SequenceEqual(theirs), what + ": ours encrypts through a stream");
    // A transform runs again from its IV after a final block, as the platform's does.
    using ICryptoTransform again = ours.CreateEncryptor();
    if (pad != PaddingMode.ISO10126) Check(again.TransformFinalBlock(data, 0, n).SequenceEqual(again.TransformFinalBlock(data, 0, n)), what + ": reusable");
    // Damaged padding is refused, as the platform refuses it.
    if (pad is PaddingMode.PKCS7 or PaddingMode.ANSIX923 && mode == CipherMode.ECB && theirs.Length >= 16)
    {
        byte[] bad = (byte[])theirs.Clone();
        bad[^1] ^= 0x55;
        Check(Try(() => real.CreateDecryptor().TransformFinalBlock(bad, 0, bad.Length)) is null
              == Try(() => ours.CreateDecryptor().TransformFinalBlock(bad, 0, bad.Length)) is null, what + ": damaged padding");
    }
}
Console.WriteLine($"{cases - failures}/{cases} checks pass");
return failures == 0 ? 0 : 1;
