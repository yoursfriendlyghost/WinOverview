// Small helpers: app icons, blurred wallpaper, taskbar visibility, monitors.
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Windows.Controls;
using System.Windows.Interop;
using System.Windows.Media.Effects;
using System.Windows.Media.Imaging;
using System.Windows.Media;
using System.Windows;
using static WinOverview.NativeMethods;

namespace WinOverview;

// ===================== IconHelper =====================
/// <summary>Gets a crisp app icon for a window (cached per .exe).</summary>
internal static class IconHelper
{
    private const int IconPx = 64;
    private static readonly Dictionary<string, ImageSource?> Cache = new(StringComparer.OrdinalIgnoreCase);
    private static readonly Dictionary<IntPtr, string?> ExeByWindow = new();
    private static readonly Dictionary<IntPtr, ImageSource?> IconByWindow = new();

    /// <summary>Forget windows that have closed (call with every window that's still open).</summary>
    public static void Prune(IEnumerable<IntPtr> openWindows)
    {
        var open = openWindows.ToHashSet();
        foreach (var h in ExeByWindow.Keys.Where(h => !open.Contains(h)).ToList()) ExeByWindow.Remove(h);
        foreach (var h in IconByWindow.Keys.Where(h => !open.Contains(h)).ToList()) IconByWindow.Remove(h);
    }

    public static ImageSource? GetIcon(IntPtr hwnd)
    {
        if (IconByWindow.TryGetValue(hwnd, out var known)) return known;
        var icon = LookUpIcon(hwnd);
        IconByWindow[hwnd] = icon;
        return icon;
    }

    private static ImageSource? LookUpIcon(IntPtr hwnd)
    {
        string? exe = GetExePath(hwnd);

        // UWP apps (Settings, Calculator…) all run inside ApplicationFrameHost.exe,
        // so its icon would be wrong — ask the window itself instead.
        bool frameHost = exe != null && exe.EndsWith("ApplicationFrameHost.exe", StringComparison.OrdinalIgnoreCase);

        if (exe != null && !frameHost)
        {
            if (!Cache.TryGetValue(exe, out var cached))
            {
                cached = FromFile(exe);
                Cache[exe] = cached;
            }
            if (cached != null) return cached;
        }
        return FromWindow(hwnd);
    }

    /// <summary>Full path of the .exe that owns a window.</summary>
    public static string? GetExePath(IntPtr hwnd)
    {
        if (ExeByWindow.TryGetValue(hwnd, out var known)) return known;
        string? path = LookUpExePath(hwnd);
        ExeByWindow[hwnd] = path;
        return path;
    }

    private static string? LookUpExePath(IntPtr hwnd)
    {
        GetWindowThreadProcessId(hwnd, out uint pid);
        IntPtr process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        if (process == IntPtr.Zero) return null;
        try
        {
            var sb = new StringBuilder(1024);
            uint size = (uint)sb.Capacity;
            return QueryFullProcessImageName(process, 0, sb, ref size) ? sb.ToString() : null;
        }
        finally
        {
            CloseHandle(process);
        }
    }

    /// <summary>64px icon from the .exe file.</summary>
    private static ImageSource? FromFile(string path)
    {
        int hr = SHDefExtractIcon(path, 0, 0, out IntPtr large, out IntPtr small, (uint)(IconPx | (16 << 16)));
        try
        {
            return hr == 0 ? ToImage(large) : null;
        }
        finally
        {
            if (large != IntPtr.Zero) DestroyIcon(large);
            if (small != IntPtr.Zero) DestroyIcon(small);
        }
    }

    /// <summary>Icon the window reports about itself (smaller, but works for UWP apps).</summary>
    private static ImageSource? FromWindow(IntPtr hwnd)
    {
        SendMessageTimeout(hwnd, WM_GETICON, (IntPtr)ICON_BIG, IntPtr.Zero, SMTO_ABORTIFHUNG, 100, out IntPtr icon);
        if (icon == IntPtr.Zero) icon = GetClassLongPtr(hwnd, GCLP_HICON);
        return ToImage(icon); // not ours to destroy
    }

    private static ImageSource? ToImage(IntPtr hIcon)
    {
        if (hIcon == IntPtr.Zero) return null;
        try
        {
            var image = Imaging.CreateBitmapSourceFromHIcon(hIcon, Int32Rect.Empty, BitmapSizeOptions.FromEmptyOptions());
            image.Freeze();
            return image;
        }
        catch
        {
            return null;
        }
    }
}

// ===================== WallpaperHelper =====================
/// <summary>Loads the current desktop wallpaper and pre-blurs it once (cached until it changes).</summary>
internal static class WallpaperHelper
{
    private const int Width = 480;      // blur a small copy — stretched up it looks smooth and costs nothing
    private const double Radius = 16;

    private static string? _cachedKey;
    private static ImageSource? _cached;

    public static ImageSource? GetBlurred()
    {
        string? path = FindWallpaper();
        if (path == null) return null;

        string key = path + "|" + File.GetLastWriteTimeUtc(path).Ticks;
        if (key == _cachedKey) return _cached;

        _cached = Load(path) is { } image ? Blur(image) : null;
        _cachedKey = key;
        return _cached;
    }

    private static string? FindWallpaper()
    {
        var sb = new StringBuilder(520);
        if (SystemParametersInfo(SPI_GETDESKWALLPAPER, (uint)sb.Capacity, sb, 0))
        {
            string path = sb.ToString();
            if (path.Length > 0 && File.Exists(path)) return path;
        }

        // Spotlight / slideshow wallpapers end up here
        string transcoded = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            @"Microsoft\Windows\Themes\TranscodedWallpaper");
        return File.Exists(transcoded) ? transcoded : null;
    }

    private static BitmapSource? Load(string path)
    {
        try
        {
            using var stream = File.OpenRead(path);
            var bmp = new BitmapImage();
            bmp.BeginInit();
            bmp.CacheOption = BitmapCacheOption.OnLoad;
            bmp.DecodePixelWidth = Width;
            bmp.StreamSource = stream;
            bmp.EndInit();
            bmp.Freeze();
            return bmp;
        }
        catch
        {
            return null; // solid-colour wallpaper or unreadable file
        }
    }

    private static BitmapSource Blur(BitmapSource src)
    {
        int w = Width;
        int h = Math.Max(1, (int)(Width * (double)src.PixelHeight / src.PixelWidth));

        // Draw slightly oversized so the blurred edges don't fade to transparent
        var image = new Image
        {
            Source = src,
            Stretch = Stretch.Fill,
            Margin = new Thickness(-2 * Radius),
            Effect = new BlurEffect { Radius = Radius, KernelType = KernelType.Gaussian },
        };
        var host = new Grid { Width = w, Height = h, ClipToBounds = true, Children = { image } };
        host.Measure(new Size(w, h));
        host.Arrange(new Rect(0, 0, w, h));

        var result = new RenderTargetBitmap(w, h, 96, 96, PixelFormats.Pbgra32);
        result.Render(host);
        result.Freeze();
        return result;
    }
}

// ===================== TaskbarHelper =====================
/// <summary>Keeps the taskbar visible on top of the overview.</summary>
internal static class TaskbarHelper
{
    /// <summary>True while we've switched auto-hide off and need to switch it back on.</summary>
    public static bool TemporarilyShown { get; private set; }

    private static APPBARDATA NewData() => new()
    {
        cbSize = Marshal.SizeOf<APPBARDATA>(),
        hWnd = FindWindow("Shell_TrayWnd", null),
    };

    private static bool IsAutoHide()
    {
        var data = NewData();
        return ((int)SHAppBarMessage(ABM_GETSTATE, ref data) & ABS_AUTOHIDE) != 0;
    }

    private static void SetState(int state)
    {
        var data = NewData();
        data.lParam = (IntPtr)state;
        SHAppBarMessage(ABM_SETSTATE, ref data);
    }

    // Marker file: if the app is killed while the overview is open, the next launch restores auto-hide.
    private static readonly string Marker = Path.Combine(Path.GetTempPath(), "WinOverview.autohide");

    /// <summary>Call once at startup.</summary>
    public static void RecoverIfNeeded()
    {
        if (!File.Exists(Marker)) return;
        SetState(ABS_AUTOHIDE);
        File.Delete(Marker);
    }

    /// <summary>
    /// Call when the overview opens. Keeps the taskbar above the overlay.
    /// If <paramref name="unhideAutoHide"/> is true and the taskbar auto-hides, auto-hide is switched
    /// off until the overview closes. (Windows then resizes maximized apps to make room — that's why
    /// it's a setting, off by default.)
    /// </summary>
    public static void Show(bool unhideAutoHide)
    {
        if (unhideAutoHide && !TemporarilyShown && IsAutoHide())
        {
            SetState(ABS_ALWAYSONTOP); // auto-hide off
            TemporarilyShown = true;
            try { File.WriteAllText(Marker, ""); } catch { /* not critical */ }
        }

        IntPtr tray = FindWindow("Shell_TrayWnd", null);
        if (tray != IntPtr.Zero)
            SetWindowPos(tray, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }

    /// <summary>Call when the overview closes (and when the app exits).</summary>
    public static void Restore()
    {
        if (!TemporarilyShown) return;
        SetState(ABS_AUTOHIDE);
        TemporarilyShown = false;
        try { File.Delete(Marker); } catch { /* not critical */ }
    }

    /// <summary>
    /// Taskbar position in physical pixels, and which screen edge it's on.
    /// An auto-hide taskbar takes no space, so it's reported as empty.
    /// </summary>
    public static (RECT Rect, int Edge) GetPosition()
    {
        if (IsAutoHide()) return (default, ABE_BOTTOM);
        var data = NewData();
        SHAppBarMessage(ABM_GETTASKBARPOS, ref data);
        return (data.rc, (int)data.uEdge);
    }
}

// ===================== MonitorHelper =====================
/// <param name="Bounds">Whole monitor, physical pixels.</param>
/// <param name="Work">Monitor minus its taskbar, physical pixels.</param>
internal readonly record struct MonitorArea(RECT Bounds, RECT Work, bool IsPrimary);

internal static class MonitorHelper
{
    /// <summary>The monitor the mouse is on.</summary>
    public static MonitorArea AtCursor()
    {
        GetCursorPos(out POINT p);
        return From(MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST));
    }

    public static MonitorArea Primary() => From(MonitorFromPoint(new POINT(), MONITOR_DEFAULTTOPRIMARY));

    private static MonitorArea From(IntPtr monitor)
    {
        var info = new MONITORINFO { cbSize = Marshal.SizeOf<MONITORINFO>() };
        GetMonitorInfo(monitor, ref info);
        return new MonitorArea(info.rcMonitor, info.rcWork, (info.dwFlags & MONITORINFOF_PRIMARY) != 0);
    }
}
