using System.Windows.Interop;
using System.Windows.Media;
using static WinOverview.NativeMethods;

namespace WinOverview;

/// <summary>
/// Draws the live window previews for "previews" mode.
///
/// Windows can't draw live previews (DWM thumbnails) on our see-through overlay, so this is a
/// separate plain window sitting just above it. It's cut (with a window region) to exactly the
/// preview rectangles, and it passes every mouse click through to the overlay underneath —
/// so the overlay still handles clicks, hover, drag and drop as usual.
/// </summary>
internal sealed class ThumbnailHost : IDisposable
{
    private readonly IntPtr _owner;
    private HwndSource? _source;
    private readonly List<Thumb> _thumbs = new();
    private IntPtr _cutoutFor;
    private int _cutoutPx;
    private bool _shown;

    private sealed class Thumb
    {
        public IntPtr Target;
        public IntPtr Handle;
        public RECT Dest;   // inside the host window, physical pixels
    }

    public ThumbnailHost(IntPtr owner) => _owner = owner;

    private IntPtr Hwnd => _source?.Handle ?? IntPtr.Zero;

    /// <param name="bounds">Where the host window goes (screen, physical pixels).</param>
    /// <param name="items">Window, where its preview goes (screen px), and which part of it to show.</param>
    /// <param name="cutoutPx">Size of the corner left uncovered for the hovered window's × button.</param>
    public void Show(RECT bounds, IEnumerable<(IntPtr Target, RECT DestScreen, RECT Source)> items, int cutoutPx)
    {
        Clear();
        EnsureWindow();
        _cutoutPx = cutoutPx;

        // Registered bottom-most first, so windows on top are drawn on top
        foreach (var (target, dest, source) in items)
        {
            if (DwmRegisterThumbnail(Hwnd, target, out IntPtr handle) != 0) continue;
            var thumb = new Thumb
            {
                Target = target,
                Handle = handle,
                Dest = new RECT(dest.Left - bounds.Left, dest.Top - bounds.Top, dest.Right - bounds.Left, dest.Bottom - bounds.Top),
            };
            _thumbs.Add(thumb);

            var props = new DWM_THUMBNAIL_PROPERTIES
            {
                dwFlags = DWM_TNP_RECTDESTINATION | DWM_TNP_RECTSOURCE | DWM_TNP_VISIBLE
                          | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
                rcDestination = thumb.Dest,
                rcSource = source,
                opacity = 255,
                fVisible = true,
                fSourceClientAreaOnly = false,
            };
            DwmUpdateThumbnailProperties(handle, ref props);
        }

        UpdateRegion();
        SetWindowPos(Hwnd, HWND_TOPMOST, bounds.Left, bounds.Top, bounds.Width, bounds.Height,
                     SWP_NOACTIVATE | SWP_SHOWWINDOW);
        _shown = true;
    }

    /// <summary>Temporarily hide the previews (while dragging, or while the snap picker is open).</summary>
    public void Suspend()
    {
        if (_shown && Hwnd != IntPtr.Zero) ShowWindow(Hwnd, SW_HIDE);
        _shown = false;
    }

    public void Resume()
    {
        if (_shown || Hwnd == IntPtr.Zero || _thumbs.Count == 0) return;
        ShowWindow(Hwnd, SW_SHOWNOACTIVATE);
        _shown = true;
    }

    /// <summary>Leave the top-right corner of this window's preview uncovered, so its × button shows.</summary>
    public void SetCutout(IntPtr target)
    {
        if (_cutoutFor == target) return;
        _cutoutFor = target;
        UpdateRegion();
    }

    /// <summary>Remove all previews and hide.</summary>
    public void Clear()
    {
        foreach (var t in _thumbs) DwmUnregisterThumbnail(t.Handle);
        _thumbs.Clear();
        _cutoutFor = IntPtr.Zero;
        Suspend();
    }

    private void EnsureWindow()
    {
        if (_source != null) return;

        var parameters = new HwndSourceParameters("WinOverviewPreviews")
        {
            WindowStyle = unchecked((int)0x80000000),                          // WS_POPUP
            ExtendedWindowStyle = (int)(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE),
            PositionX = 0,
            PositionY = 0,
            Width = 1,
            Height = 1,
        };
        _source = new HwndSource(parameters);
        _source.AddHook(Hook);
        if (_source.CompositionTarget != null)
            _source.CompositionTarget.BackgroundColor = Color.FromRgb(0x22, 0x22, 0x2C);

        // Owned by the overlay → always sits just above it
        SetWindowLongPtr(Hwnd, GWLP_HWNDPARENT, _owner);
    }

    private static IntPtr Hook(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled)
    {
        switch (msg)
        {
            case WM_NCHITTEST:            // clicks go through to the overlay (same thread)
                handled = true;
                return (IntPtr)HTTRANSPARENT;
            case WM_MOUSEACTIVATE:        // and never take focus
                handled = true;
                return (IntPtr)MA_NOACTIVATE;
        }
        return IntPtr.Zero;
    }

    /// <summary>Shape the window to just the previews (minus the hovered × corner).</summary>
    private void UpdateRegion()
    {
        if (Hwnd == IntPtr.Zero) return;

        IntPtr region = CreateRectRgn(0, 0, 0, 0);
        foreach (var t in _thumbs)
        {
            IntPtr part = CreateRectRgn(t.Dest.Left, t.Dest.Top, t.Dest.Right, t.Dest.Bottom);
            CombineRgn(region, region, part, RGN_OR);
            DeleteObject(part);
        }
        foreach (var t in _thumbs.Where(t => t.Target == _cutoutFor))
        {
            IntPtr corner = CreateRectRgn(t.Dest.Right - _cutoutPx, t.Dest.Top, t.Dest.Right, t.Dest.Top + _cutoutPx);
            CombineRgn(region, region, corner, RGN_DIFF);
            DeleteObject(corner);
        }
        SetWindowRgn(Hwnd, region, true); // Windows owns the region from here
    }

    public void Dispose()
    {
        Clear();
        _source?.Dispose();
        _source = null;
    }
}
