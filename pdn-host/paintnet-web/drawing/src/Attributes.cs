// ImageAttributes' colour matrix, which Graphics.DrawImage applies to the image's straight colour before drawing it, as
// GDI+ does: [r g b a 1] times the 5×5 matrix, each result clamped to 0..1. The wrap mode is kept and not used (an image
// is sampled clamped at its edges); the other adjustments (gamma, threshold, colour keys, remap tables) stay stubs.
namespace System.Drawing.Imaging
{
    public sealed partial class ColorMatrix
    {
        private readonly float[,] m = new float[5, 5];

        public ColorMatrix()
        {
            for (int i = 0; i < 5; i++) m[i, i] = 1;
        }

        public ColorMatrix(float[][] newColorMatrix)
        {
            for (int i = 0; i < 5; i++)
                for (int j = 0; j < 5; j++)
                    m[i, j] = newColorMatrix[i][j];
        }

        public ColorMatrix(ReadOnlySpan<float> newColorMatrix)
        {
            if (newColorMatrix.Length < 25) throw new ArgumentException("a colour matrix has 25 values", nameof(newColorMatrix));
            for (int i = 0; i < 25; i++) m[i / 5, i % 5] = newColorMatrix[i];
        }

        public float this[int row, int column] { get => m[row, column]; set => m[row, column] = value; }

        public float Matrix00 { get => m[0, 0]; set => m[0, 0] = value; }
        public float Matrix01 { get => m[0, 1]; set => m[0, 1] = value; }
        public float Matrix02 { get => m[0, 2]; set => m[0, 2] = value; }
        public float Matrix03 { get => m[0, 3]; set => m[0, 3] = value; }
        public float Matrix04 { get => m[0, 4]; set => m[0, 4] = value; }
        public float Matrix10 { get => m[1, 0]; set => m[1, 0] = value; }
        public float Matrix11 { get => m[1, 1]; set => m[1, 1] = value; }
        public float Matrix12 { get => m[1, 2]; set => m[1, 2] = value; }
        public float Matrix13 { get => m[1, 3]; set => m[1, 3] = value; }
        public float Matrix14 { get => m[1, 4]; set => m[1, 4] = value; }
        public float Matrix20 { get => m[2, 0]; set => m[2, 0] = value; }
        public float Matrix21 { get => m[2, 1]; set => m[2, 1] = value; }
        public float Matrix22 { get => m[2, 2]; set => m[2, 2] = value; }
        public float Matrix23 { get => m[2, 3]; set => m[2, 3] = value; }
        public float Matrix24 { get => m[2, 4]; set => m[2, 4] = value; }
        public float Matrix30 { get => m[3, 0]; set => m[3, 0] = value; }
        public float Matrix31 { get => m[3, 1]; set => m[3, 1] = value; }
        public float Matrix32 { get => m[3, 2]; set => m[3, 2] = value; }
        public float Matrix33 { get => m[3, 3]; set => m[3, 3] = value; }
        public float Matrix34 { get => m[3, 4]; set => m[3, 4] = value; }
        public float Matrix40 { get => m[4, 0]; set => m[4, 0] = value; }
        public float Matrix41 { get => m[4, 1]; set => m[4, 1] = value; }
        public float Matrix42 { get => m[4, 2]; set => m[4, 2] = value; }
        public float Matrix43 { get => m[4, 3]; set => m[4, 3] = value; }
        public float Matrix44 { get => m[4, 4]; set => m[4, 4] = value; }

        /// The matrix applied to straight BGRA bytes in place.
        internal void Apply(Span<byte> bgra)
        {
            Span<float> v = stackalloc float[4];
            for (int p = 0; p < bgra.Length; p += 4)
            {
                float r = bgra[p + 2] / 255f, g = bgra[p + 1] / 255f, b = bgra[p] / 255f, a = bgra[p + 3] / 255f;
                for (int j = 0; j < 4; j++)
                    v[j] = r * m[0, j] + g * m[1, j] + b * m[2, j] + a * m[3, j] + m[4, j];
                bgra[p + 2] = Byte(v[0]); bgra[p + 1] = Byte(v[1]); bgra[p] = Byte(v[2]); bgra[p + 3] = Byte(v[3]);
            }
            static byte Byte(float f) => (byte)(Math.Clamp(f, 0, 1) * 255 + 0.5f);
        }
    }

    public sealed partial class ImageAttributes
    {
        internal ColorMatrix Matrix;

        public ImageAttributes() { }
        public void Dispose() { }
        public object Clone() => new ImageAttributes { Matrix = Matrix };
        public void SetColorMatrix(ColorMatrix newColorMatrix) => Matrix = newColorMatrix;
        // ponytail: SkipGrays and the per-type (bitmap/brush/pen) matrices are not told apart; every image gets the matrix.
        public void SetColorMatrix(ColorMatrix newColorMatrix, ColorMatrixFlag flags) => Matrix = newColorMatrix;
        public void SetColorMatrix(ColorMatrix newColorMatrix, ColorMatrixFlag mode, ColorAdjustType type) => Matrix = newColorMatrix;
        public void ClearColorMatrix() => Matrix = null;
        public void ClearColorMatrix(ColorAdjustType type) => Matrix = null;
        public void SetWrapMode(Drawing2D.WrapMode mode) { }
        public void SetWrapMode(Drawing2D.WrapMode mode, Color color) { }
        public void SetWrapMode(Drawing2D.WrapMode mode, Color color, bool clamp) { }

        /// The image with these attributes applied: itself, or a recoloured copy.
        internal Image Applied(Image image)
        {
            if (Matrix is null || image is not Bitmap bmp) return image;
            var copy = new Bitmap(bmp);
            Matrix.Apply(copy.bgra);
            return copy;
        }
    }
}

// GDI+'s bitmap effects (Bitmap.ApplyEffect). The colour matrix one is here, as ImageAttributes' matrix applies it;
// the others (blur, sharpen, curves, lookup tables, red-eye) stay stubs.
namespace System.Drawing.Imaging.Effects
{
    public abstract partial class Effect
    {
        public void Dispose() { Dispose(true); GC.SuppressFinalize(this); }
        protected virtual void Dispose(bool disposing) { }

        /// The effect over `area` of a straight BGRA image `width` pixels wide, in place.
        internal virtual void Apply(byte[] bgra, int width, Rectangle area)
            => throw Unsupported.Member(GetType().FullName + " in Bitmap.ApplyEffect");
    }

    public partial class ColorMatrixEffect
    {
        private readonly ColorMatrix matrix = null!;
        public ColorMatrixEffect(ColorMatrix matrix) : base() => this.matrix = matrix;
        public ColorMatrix Matrix => matrix;

        internal override void Apply(byte[] bgra, int width, Rectangle area)
        {
            for (int y = area.Top; y < area.Bottom; y++)
                matrix.Apply(bgra.AsSpan((y * width + area.Left) * 4, area.Width * 4));
        }
    }
}

namespace System.Drawing
{
    public sealed partial class Bitmap
    {
        /// The effect over `area` (the whole bitmap when it is empty), clipped to the bitmap.
        public void ApplyEffect(Imaging.Effects.Effect effect, Rectangle area = default)
        {
            var bounds = new Rectangle(0, 0, Width, Height);
            area = area.IsEmpty ? bounds : Rectangle.Intersect(area, bounds);
            if (area.Width > 0 && area.Height > 0) effect.Apply(bgra, width, area);
        }
    }
}
