// Virtual desktops (via VirtualDesktopAccessor.dll) and the list of app windows on them.
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using static WinOverview.NativeMethods;

namespace WinOverview;

// ===================== VirtualDesktop =====================
/// <summary>
/// Windows has no public API to move windows between virtual desktops,
/// so we use VirtualDesktopAccessor.dll (github.com/Ciantic/VirtualDesktopAccessor).
/// The DLL is embedded inside WinOverview.exe, so users only need the one file:
/// on first use it's unpacked to %LocalAppData%\WinOverview and loaded from there.
/// All functions return -1 on error. Desktop numbers start at 0.
/// </summary>
internal static class VirtualDesktop
{
    private const string Dll = "VirtualDesktopAccessor.dll";

    // Runs before the first call into the DLL: tells .NET where to find it
    static VirtualDesktop()
    {
        NativeLibrary.SetDllImportResolver(typeof(VirtualDesktop).Assembly, (name, _, _) =>
            name == Dll && TryLoad(out IntPtr handle) ? handle : IntPtr.Zero);
    }

    private static bool TryLoad(out IntPtr handle)
    {
        // 1. Next to the .exe (normal developer builds)
        string beside = Path.Combine(AppContext.BaseDirectory, Dll);
        if (File.Exists(beside) && NativeLibrary.TryLoad(beside, out handle)) return true;

        // 2. The copy embedded in the .exe (the single-file release)
        string? unpacked = UnpackEmbedded();
        handle = IntPtr.Zero;
        return unpacked != null && NativeLibrary.TryLoad(unpacked, out handle);
    }

    private static string? UnpackEmbedded()
    {
        using Stream? embedded = typeof(VirtualDesktop).Assembly.GetManifestResourceStream(Dll);
        if (embedded == null) return null;

        string folder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "WinOverview");
        string path = Path.Combine(folder, Dll);
        try
        {
            Directory.CreateDirectory(folder);
            if (!File.Exists(path) || new FileInfo(path).Length != embedded.Length)
            {
                using var file = File.Create(path);
                embedded.CopyTo(file);
            }
        }
        catch (IOException)
        {
            // Already there and in use by another copy — that's fine
        }
        return File.Exists(path) ? path : null;
    }

    [DllImport(Dll)] public static extern int GetDesktopCount();
    [DllImport(Dll)] public static extern int GetCurrentDesktopNumber();
    [DllImport(Dll)] public static extern int GoToDesktopNumber(int number);
    [DllImport(Dll)] public static extern int GetWindowDesktopNumber(IntPtr hwnd);
    [DllImport(Dll)] public static extern int MoveWindowToDesktopNumber(IntPtr hwnd, int number);
    [DllImport(Dll)] public static extern int IsPinnedWindow(IntPtr hwnd);
    [DllImport(Dll)] public static extern int PinWindow(IntPtr hwnd);
    [DllImport(Dll)] public static extern int CreateDesktop();
    [DllImport(Dll)] public static extern int RemoveDesktop(int remove, int fallback);

    /// <summary>Checks the DLL is present and working before we rely on it.</summary>
    public static bool IsAvailable(out string error)
    {
        try
        {
            error = "";
            if (GetDesktopCount() > 0) return true;
            error = "VirtualDesktopAccessor.dll loaded but returned no desktops.\n" +
                    "Your Windows build may need a different release of the DLL.";
            return false;
        }
        catch (DllNotFoundException)
        {
            error = "VirtualDesktopAccessor.dll couldn't be loaded.\n\n" +
                    "If you're building from source, download it from\n" +
                    "github.com/Ciantic/VirtualDesktopAccessor/releases and put it in the project folder.";
            return false;
        }
        catch (EntryPointNotFoundException)
        {
            error = "VirtualDesktopAccessor.dll is an old version. Download the latest release.";
            return false;
        }
    }
}

// ===================== WindowEnumerator =====================
/// <param name="Desktop">Virtual desktop number (0-based).</param>
/// <param name="Rect">Window position on screen, in physical pixels.</param>
internal sealed record WindowInfo(IntPtr Handle, string Title, int Desktop, RECT Rect, bool Minimized);

/// <summary>Lists app windows (like Alt+Tab) across ALL virtual desktops, top-most first.</summary>
internal static class WindowEnumerator
{
    public static List<WindowInfo> GetWindows(IntPtr exclude, int currentDesktop)
    {
        var list = new List<WindowInfo>();

        EnumWindows((h, _) =>
        {
            if (h == exclude || !IsWindowVisible(h)) return true;
            if (GetWindow(h, GW_OWNER) != IntPtr.Zero) return true;            // dialogs/popups

            long exStyle = GetWindowLongPtr(h, GWL_EXSTYLE).ToInt64();
            if ((exStyle & WS_EX_TOOLWINDOW) != 0) return true;                // tool palettes

            int desktop = VirtualDesktop.GetWindowDesktopNumber(h);
            bool pinned = VirtualDesktop.IsPinnedWindow(h) == 1;

            // Cloaked by the shell = sitting on another virtual desktop (keep it).
            // Any other cloak = hidden UWP/background window (skip it).
            if (DwmGetWindowAttribute(h, DWMWA_CLOAKED, out int cloaked, sizeof(int)) == 0 && cloaked != 0)
            {
                if (cloaked != DWM_CLOAKED_SHELL || desktop < 0) return true;
            }

            if (pinned || desktop < 0) desktop = currentDesktop; // pinned = shown on every desktop

            int len = GetWindowTextLength(h);
            if (len == 0) return true;

            var cls = new StringBuilder(256);
            GetClassName(h, cls, cls.Capacity);
            if (cls.ToString() == "Progman") return true;                      // the desktop itself

            var title = new StringBuilder(len + 1);
            GetWindowText(h, title, title.Capacity);

            GetWindowRect(h, out RECT rect);
            list.Add(new WindowInfo(h, title.ToString(), desktop, rect, IsIconic(h)));
            return true;
        }, IntPtr.Zero);

        return list;
    }
}
