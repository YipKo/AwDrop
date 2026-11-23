using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using Winawdl.Theme;

namespace Winawdl.Shell;

/// <summary>Makes Windows draw the title bar in the table's color, so the window
/// is one surface with the system's own buttons on it.</summary>
public static class TitleBar
{
    private const int UseDarkMode = 20, BorderColor = 34, CaptionColor = 35, TextColor = 36;

    public static void Paint(Window window, Palette palette)
    {
        var handle = new WindowInteropHelper(window).Handle;
        if (handle == IntPtr.Zero || palette.Plain) return;
        var dark = palette.Dark ? 1 : 0;
        DwmSetWindowAttribute(handle, UseDarkMode, ref dark, sizeof(int));
        var table = ColorRef(palette.Table);
        DwmSetWindowAttribute(handle, CaptionColor, ref table, sizeof(int));
        DwmSetWindowAttribute(handle, BorderColor, ref table, sizeof(int));
        var ink = ColorRef(palette.Ink);
        DwmSetWindowAttribute(handle, TextColor, ref ink, sizeof(int));
    }

    private static int ColorRef(Rgb color) => color.R | color.G << 8 | color.B << 16;

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr window, int attribute, ref int value, int size);
}

/// <summary>Two cards show a nearby transfer, with radio waves on the sending card.</summary>
public static class AppIcon
{
    public static readonly Color Ground = Color.FromRgb(0xF3, 0xE9, 0xDE);

    private const double Grid = 256;
    private static readonly Geometry Plate = new RectangleGeometry(new Rect(8, 8, 240, 240), 58, 58);
    private static readonly Geometry BackCard = new RectangleGeometry(new Rect(49, 68, 116, 81), 20, 20);
    private static readonly Geometry FrontCard = new RectangleGeometry(new Rect(88, 104, 119, 84), 21, 21);
    private static readonly Geometry OuterWave = Geometry.Parse("M70,105 Q82,82 107,82");
    private static readonly Geometry InnerWave = Geometry.Parse("M82,111 Q90,94 107,95");
    private static readonly Geometry Transfer = Geometry.Parse(
        "M122,139 H150 L144,132 Q140,127 145,123 Q150,119 154,124 L171,141 " +
        "Q176,146 171,151 L154,168 Q150,173 145,169 Q140,165 144,160 " +
        "L150,153 H122 Q115,153 115,146 Q115,139 122,139 Z");
    private static readonly Geometry TrayMark = new CombinedGeometry(GeometryCombineMode.Exclude,
        new CombinedGeometry(GeometryCombineMode.Union, BackCard, FrontCard), Transfer);

    static AppIcon()
    {
        Plate.Freeze();
        BackCard.Freeze();
        FrontCard.Freeze();
        OuterWave.Freeze();
        InnerWave.Freeze();
        Transfer.Freeze();
        TrayMark.Freeze();
    }

    /// <summary>The mark at `size` pixels, on its plate, or alone when `ground` is null.</summary>
    public static RenderTargetBitmap Draw(int size, Color mark, Color? ground)
    {
        var visual = new DrawingVisual();
        using (var dc = visual.RenderOpen())
        {
            dc.PushTransform(new ScaleTransform(size / Grid, size / Grid));
            if (ground is { } fill)
            {
                var plateBrush = new LinearGradientBrush(Color.FromRgb(0xFF, 0xFC, 0xF6),
                    Color.FromRgb(0xDA, 0xC9, 0xBA), new Point(0, 0), new Point(1, 1));
                plateBrush.GradientStops.Insert(1, new GradientStop(fill, 0.5));
                dc.DrawGeometry(plateBrush, new Pen(new SolidColorBrush(Color.FromRgb(0xD4, 0xC2, 0xB2)), 1.5), Plate);
            }

            dc.PushTransform(new RotateTransform(-10, Grid / 2, Grid / 2));
            if (ground is null)
            {
                dc.DrawGeometry(new SolidColorBrush(mark), null, TrayMark);
            }
            else
            {
                var back = new LinearGradientBrush(Color.FromRgb(0xFF, 0xBC, 0x73), Color.FromRgb(0xCF, 0x59, 0x30), 90);
                var front = new LinearGradientBrush(Color.FromRgb(0xEC, 0x78, 0x43), Color.FromRgb(0xAE, 0x37, 0x20), 90);
                dc.DrawGeometry(back, new Pen(new SolidColorBrush(Color.FromArgb(78, 255, 247, 223)), 1.5), BackCard);
                if (size >= 24)
                    DrawWaves(dc, size);
                if (size >= 48)
                {
                    dc.PushTransform(new TranslateTransform(0, 3));
                    dc.DrawGeometry(new SolidColorBrush(Color.FromArgb(36, 117, 44, 21)), null, FrontCard);
                    dc.Pop();
                }
                dc.DrawGeometry(front, new Pen(new SolidColorBrush(Color.FromArgb(80, 255, 207, 169)), 1.5), FrontCard);
                dc.DrawGeometry(new SolidColorBrush(mark), null, Transfer);
            }
            dc.Pop();
            dc.Pop();
        }
        var bitmap = new RenderTargetBitmap(size, size, 96, 96, PixelFormats.Pbgra32);
        bitmap.Render(visual);
        bitmap.Freeze();
        return bitmap;
    }

    private static void DrawWaves(DrawingContext dc, int size)
    {
        var pen = new Pen(new SolidColorBrush(Color.FromRgb(0xFF, 0xF4, 0xDE)), size >= 48 ? 6 : 9)
        {
            StartLineCap = PenLineCap.Round,
            EndLineCap = PenLineCap.Round,
        };
        dc.DrawGeometry(null, pen, OuterWave);
        if (size >= 48)
            dc.DrawGeometry(null, pen, InnerWave);
    }

    /// <summary>For the notification area, which shows small marks in one color.</summary>
    public static System.Drawing.Icon ForTray(bool onDarkTaskbar)
    {
        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(Draw(32, onDarkTaskbar ? Colors.White : Colors.Black, null)));
        using var png = new System.IO.MemoryStream();
        encoder.Save(png);
        png.Position = 0;
        using var bitmap = new System.Drawing.Bitmap(png);
        // An icon made from a handle does not free it; its copy owns one that it does.
        var handle = bitmap.GetHicon();
        using var borrowed = System.Drawing.Icon.FromHandle(handle);
        var owned = (System.Drawing.Icon)borrowed.Clone();
        DestroyIcon(handle);
        return owned;
    }

    [DllImport("user32.dll")]
    private static extern bool DestroyIcon(IntPtr icon);
}
