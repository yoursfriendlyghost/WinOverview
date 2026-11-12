using System.Windows;
using System.Windows.Controls;
using System.Windows.Interop;
using System.Windows.Media;
using static WinOverview.NativeMethods;

namespace WinOverview;

/// <summary>
/// Small floating window that shows a live preview of an app when you hover its icon.
/// It's a separate, non-transparent window because Windows can't draw live previews
/// (DWM thumbnails) on the main overlay, which uses transparency.
/// </summary>
internal sealed class PreviewPopup : Window
{
    private const double PopupW = 320;
    private const double TitleH = 30;
    private const double Pad = 8;

    private readonly TextBlock _title;
    private readonly IntPtr _hwnd;
    private IntPtr _thumb;

    public PreviewPopup(Window owner, IntPtr ownerHwnd)
    {
        // Owned by the overlay (so it always sits above it). Set through the HWND, because
        // WPF's Owner property refuses an owner that hasn't been shown yet.
        new WindowInteropHelper(this).Owner = ownerHwnd;
        WindowStyle = WindowStyle.None;
        ResizeMode = ResizeMode.NoResize;
        ShowInTaskbar = false;
        ShowActivated = false;   // never steal focus from the overview
        Topmost = true;
        Background = new SolidColorBrush(Color.FromRgb(0x1C, 0x1C, 0x24));
        FontFamily = owner.FontFamily;
        Width = PopupW;
        Height = 200;

        _title = new TextBlock
        {
            Foreground = Brushes.White,
            FontSize = 12,
            Margin = new Thickness(10, 8, 10, 0),
            TextTrimming = TextTrimming.CharacterEllipsis,
            VerticalAlignment = VerticalAlignment.Top,
        };
        Content = new Grid { Children = { _title } };

        _hwnd = new WindowInteropHelper(this).EnsureHandle();

        // Clicks never activate it, and it never appears in Alt+Tab
        long ex = GetWindowLongPtr(_hwnd, GWL_EXSTYLE).ToInt64();
        SetWindowLongPtr(_hwnd, GWL_EXSTYLE, (IntPtr)(ex | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW));

        // Windows 11 rounded corners + dark frame
        int round = DWMWCP_ROUND;
        DwmSetWindowAttribute(_hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, ref round, sizeof(int));
        int dark = 1;
        DwmSetWindowAttribute(_hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, ref dark, sizeof(int));
    }

    /// <summary>Show a live preview of <paramref name="target"/> just above (or below) the anchor, in screen pixels.</summary>
    public void ShowFor(IntPtr target, string title, Rect anchorPx, RECT monitorPx, double scale)
    {
        RemoveThumb();
        if (IsIconic(target) || !GetWindowRect(target, out RECT wr) || wr.Width <= 0 || wr.Height <= 0)
        {
            HidePreview();
            return; // minimized windows have nothing to show
        }

        // Size: fixed width, height follows the window's shape
        double areaW = PopupW - 2 * Pad;
        double areaH = Math.Clamp(areaW * wr.Height / wr.Width, 90, 220);
        double popupH = TitleH + areaH + Pad;

        int wPx = (int)Math.Round(PopupW * scale);
        int hPx = (int)Math.Round(popupH * scale);
        int gapPx = (int)Math.Round(10 * scale);

        int x = (int)Math.Round(anchorPx.X + anchorPx.Width / 2 - wPx / 2.0);
        x = Math.Clamp(x, monitorPx.Left + gapPx, Math.Max(monitorPx.Left + gapPx, monitorPx.Right - wPx - gapPx));
        int y = (int)Math.Round(anchorPx.Y) - hPx - gapPx;
        if (y < monitorPx.Top + gapPx) y = (int)Math.Round(anchorPx.Bottom) + gapPx; // no room above → go below

        _title.Text = title;
        if (!IsVisible) Show();
        SetWindowPos(_hwnd, HWND_TOPMOST, x, y, wPx, hPx, SWP_NOACTIVATE);

        if (DwmRegisterThumbnail(_hwnd, target, out _thumb) != 0)
        {
            _thumb = IntPtr.Zero;
            return;
        }

        // Fit the window inside the preview area, keeping its shape
        double ar = (double)wr.Width / wr.Height;
        double tw = areaW, th = areaW / ar;
        if (th > areaH) { th = areaH; tw = areaH * ar; }
        double tx = Pad + (areaW - tw) / 2;
        double ty = TitleH + (areaH - th) / 2;

        var props = new DWM_THUMBNAIL_PROPERTIES
        {
            dwFlags = DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination = new RECT(
                (int)Math.Round(tx * scale), (int)Math.Round(ty * scale),
                (int)Math.Round((tx + tw) * scale), (int)Math.Round((ty + th) * scale)),
            opacity = 255,
            fVisible = true,
            fSourceClientAreaOnly = false,
        };
        DwmUpdateThumbnailProperties(_thumb, ref props);
    }

    public void HidePreview()
    {
        RemoveThumb();
        if (IsVisible) Hide();
    }

    private void RemoveThumb()
    {
        if (_thumb == IntPtr.Zero) return;
        DwmUnregisterThumbnail(_thumb);
        _thumb = IntPtr.Zero;
    }
}
