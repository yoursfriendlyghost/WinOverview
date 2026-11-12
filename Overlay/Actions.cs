using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Media.Effects;
using System.Windows.Shapes;
using System.Windows.Threading;
using static WinOverview.NativeMethods;

namespace WinOverview;

// What clicks and keys end up doing: switch desktop, focus/close apps, add/remove desktops.
public partial class MainWindow
{
    // ================= Actions =================

    private void SwitchTo(int desktop)
    {
        if (desktop < 0 || desktop >= VirtualDesktop.GetDesktopCount()) return;
        HideOverview(() => VirtualDesktop.GoToDesktopNumber(desktop));
    }

    private void FocusWindow(WindowInfo w)
    {
        HideOverview(() =>
        {
            if (w.Desktop != _current) VirtualDesktop.GoToDesktopNumber(w.Desktop);
            if (IsIconic(w.Handle)) ShowWindow(w.Handle, SW_RESTORE);
            SetForegroundWindow(w.Handle);
        });
    }

    private void CloseWindow(WindowInfo w)
    {
        if (_closing) return;
        HidePreview();
        PostMessage(w.Handle, WM_CLOSE, IntPtr.Zero, IntPtr.Zero);
        Refresh(300); // the app may ask "save changes?" first
    }

    /// <summary>Moves the desktop's apps to desktop 1 (or 2, if closing desktop 1), then removes it.</summary>
    private void CloseDesktop(int index)
    {
        if (_closing || VirtualDesktop.GetDesktopCount() <= 1) return;
        int fallback = index == 0 ? 1 : 0;

        foreach (var w in _windows.Where(w => w.Desktop == index))
        {
            if (VirtualDesktop.IsPinnedWindow(w.Handle) == 1) continue; // already on every desktop
            VirtualDesktop.MoveWindowToDesktopNumber(w.Handle, fallback);
        }
        VirtualDesktop.RemoveDesktop(index, fallback);
        Refresh(150);
    }

    /// <summary>Creates a desktop and returns its number (new desktops go at the end).</summary>
    private static int CreateDesktop()
    {
        int before = VirtualDesktop.GetDesktopCount();
        return VirtualDesktop.CreateDesktop() >= 0 ? before : -1;
    }
}
