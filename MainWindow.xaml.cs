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

public partial class MainWindow : Window
{
    // ---- Layout ----
    private const int HotkeyId = 1;
    private const int GridCols = 3;            // icon grid inside a desktop: 3 × 2
    private const int GridRows = 2;
    private const double Cell = 46;            // icon square size
    private const double Gap = 6;
    private const double IconSize = 28;
    private const double TilePad = 12;
    private const double PillHeight = 20;
    private const double DragThreshold = 6;

    private const double GridW = GridCols * Cell + (GridCols - 1) * Gap;
    private const double GridH = GridRows * Cell + (GridRows - 1) * Gap;
    private const double Outline = 2;            // hover outline around a live preview
    private const double PreviewSlotW = 170;     // one live thumbnail's slot width in "previews" mode
    private const double PreviewGap = 8;
    private const int PreviewMaxCols = 3, PreviewMaxRows = 3;

    // Tile size depends on the mode: icon grid, or a mini-screen shaped like your monitor
    private static bool _previewLayout;
    private static double _screenAspect = 9.0 / 16;
    private static double PreviewSlotH => Math.Round(PreviewSlotW * _screenAspect);
    // Base tile size (widgets, the "+" tile, and desktops in icon view). In "previews" view,
    // desktops grow with their number of apps — see PreviewGridSize.
    private static double BodyW => _previewLayout ? PreviewSlotW : GridW;
    private static double BodyH => _previewLayout ? PreviewSlotH : GridH;
    private static double TileW => BodyW + 2 * TilePad;
    private static double TileH => PillHeight + 10 + BodyH + 2 * TilePad;

    // ---- Snap picker ----
    private const double PreviewW = 112;
    private const double PreviewH = 66;

    // ---- Animation (ms) ----
    private const double OpenMs = 200;
    private const double CloseMs = 120;
    private const double FlyMs = 280;

    // ---- Colours (accent ones come from config.json) ----
    private static readonly Brush TileBg = Frozen(0xFF, 0x22, 0x22, 0x2C);
    private static readonly Brush TileHoverBg = Frozen(0xFF, 0x2A, 0x2A, 0x36);
    private static readonly Brush CellBg = Frozen(0xFF, 0x30, 0x30, 0x3C);
    private static readonly Brush CellHoverBg = Frozen(0xFF, 0x3E, 0x3E, 0x4C);
    private static readonly Brush PillBg = Frozen(0x22, 0xFF, 0xFF, 0xFF);
    private static readonly Brush PillText = Frozen(0xCC, 0xFF, 0xFF, 0xFF);
    private static readonly Brush AccentText = Frozen(0xFF, 0x10, 0x18, 0x28);
    private static readonly Brush FaintText = Frozen(0x55, 0xFF, 0xFF, 0xFF);
    private static readonly Brush NewTileBorder = Frozen(0x2A, 0xFF, 0xFF, 0xFF);
    private static readonly Brush LetterBg = Frozen(0xFF, 0x4A, 0x4A, 0x5A);
    private static readonly Brush CloseBg = Frozen(0xE6, 0x44, 0x44, 0x52);
    private static readonly Brush CloseHoverBg = Frozen(0xFF, 0xE8, 0x48, 0x55);
    private static readonly Brush ZoneBg = Frozen(0x2E, 0xFF, 0xFF, 0xFF);
    private static readonly Brush PreviewBg = Frozen(0xFF, 0x20, 0x20, 0x2A);
    private static readonly Brush PreviewHoverBg = Frozen(0xFF, 0x2A, 0x2A, 0x38);
    private static Brush Accent = Frozen(0xFF, 0x8A, 0xB4, 0xF8);
    private static Brush AccentSoft = Frozen(0x55, 0x8A, 0xB4, 0xF8);

    // Snap layouts: zones as fractions of the screen (x, y, width, height)
    private static readonly Rect[][] SnapLayouts =
    {
        new[] { new Rect(0, 0, .5, 1), new Rect(.5, 0, .5, 1) },
        new[] { new Rect(0, 0, 2.0 / 3, 1), new Rect(2.0 / 3, 0, 1.0 / 3, 1) },
        new[] { new Rect(0, 0, .5, 1), new Rect(.5, 0, .5, .5), new Rect(.5, .5, .5, .5) },
        new[] { new Rect(0, 0, .5, .5), new Rect(.5, 0, .5, .5), new Rect(0, .5, .5, .5), new Rect(.5, .5, .5, .5) },
        new[] { new Rect(0, 0, 1.0 / 3, 1), new Rect(1.0 / 3, 0, 1.0 / 3, 1), new Rect(2.0 / 3, 0, 1.0 / 3, 1) },
        new[] { new Rect(0, 0, .25, 1), new Rect(.25, 0, .5, 1), new Rect(.75, 0, .25, 1) },
        new[] { new Rect(0, 0, .5, .5), new Rect(0, .5, .5, .5), new Rect(.5, 0, .5, 1) },
        new[] { new Rect(0, 0, 1, .5), new Rect(0, .5, 1, .5) },
        new[] { new Rect(0, 0, 1, 1) }, // full screen
    };

    private readonly IntPtr _hwnd;
    private readonly ScaleTransform _cardScale = new(1, 1);
    private readonly PreviewPopup? _preview;
    private readonly DispatcherTimer _hoverTimer = new();
    private AppConfig _config = new();
    private bool _pinned;
    private bool _closing;
    private int _current;
    private IntPtr _previousForeground;        // the app you were in when the overview opened
    private IntPtr _justMoved;                 // app that just landed on a new desktop (gets a pop-in)
    private RECT _monitorPx;                   // monitor the overview is on
    private RECT _snapAreaPx;                  // where snapped windows go
    private List<WindowInfo> _windows = new();

    private readonly List<DesktopTile> _tiles = new();

    // Drag / hover state
    private IconItem? _pressed;
    private IconItem? _hovered;
    private Point _pressPoint;
    private bool _dragging;
    private FrameworkElement? _ghost;
    private FrameworkElement? _flyout;

    // "previews" mode
    private ThumbnailHost? _thumbHost;
    private readonly List<IconItem> _previewItems = new();
    private bool _openAnimDone;

    // Hotkey
    private string? _registeredHotkey;
    private bool _hotkeyPaused;

    public bool FailedToStart { get; }
    public string HotkeyText { get; private set; } = "Alt+`";

    /// <summary>A Pomodoro phase ended (message for the user).</summary>
    public event Action<string>? PomodoroFinished;

    // ---- Widgets ----
    private readonly Pomodoro _pomodoro = new();
    private readonly DispatcherTimer _widgetTimer = new() { Interval = TimeSpan.FromSeconds(1) };
    private TextBlock? _pomoTime, _pomoPill, _pomoPlayGlyph, _pomoRounds;
    private Border? _pomoPillBorder, _pomoBar;
    private TextBlock? _musicTitle, _musicArtist, _musicPlayGlyph;
    private Grid? _vinyl;
    private Ellipse? _vinylLabel;
    private Storyboard? _vinylSpin;
    private bool _vinylSpinning;
    private MediaInfo _media = MediaInfo.None;
    private FrameworkElement? _musicControls;
    private bool _musicBusy;

    private sealed class DesktopTile
    {
        public int Index;            // desktop number, or -1 for the "+" tile
        public bool IsCurrent;
        public Border Root = null!;
        public bool IsNew => Index < 0;
    }

    private sealed class IconItem
    {
        public WindowInfo Info = null!;
        public ImageSource? Icon;
        public Border Cell = null!;
        public bool IsPreview;       // a live mini-window ("previews" mode) rather than an icon
        public RECT Source;          // which part of the window the preview shows

        // Keyboard navigation / search
        public bool Matches = true;  // matches the current search
        public string? ExeName;      // looked up the first time it's searched
        public int IndexInDesktop;   // position in its desktop's grid…
        public int DesktopCols = 1;  // …and how many columns that grid has
    }

    public MainWindow(AppConfig config)
    {
        InitializeComponent();
        Card.RenderTransform = _cardScale;

        if (!VirtualDesktop.IsAvailable(out string error))
        {
            FailedToStart = true;
            MessageBox.Show(error, "WinOverview");
            return;
        }

        TaskbarHelper.RecoverIfNeeded(); // in case the last run was killed mid-overview

        _hwnd = new WindowInteropHelper(this).EnsureHandle();
        HwndSource.FromHwnd(_hwnd).AddHook(WndProc);
        Root.Background = Frozen(0x01, 0, 0, 0);         // nearly invisible, but catches clicks

        _preview = new PreviewPopup(this, _hwnd);
        _hoverTimer.Tick += (_, _) =>
        {
            _hoverTimer.Stop();
            if (_hovered != null && _pressed == null && _flyout == null && !_closing) ShowPreview(_hovered);
        };

        _pomodoro.Finished += message => PomodoroFinished?.Invoke(message);
        _widgetTimer.Tick += (_, _) => UpdateWidgets();

        // Screens added/removed/resized, or the PC going to sleep: close the overview, it's laid out for the old screen
        Microsoft.Win32.SystemEvents.DisplaySettingsChanged += (_, _) => Dispatcher.InvokeAsync(() => HideOverview(animate: false));
        Microsoft.Win32.SystemEvents.PowerModeChanged += (_, _) => Dispatcher.InvokeAsync(() => HideOverview(animate: false));

        ApplyConfig(config);
    }

    private static Brush Frozen(byte a, byte r, byte g, byte b) => Frozen(Color.FromArgb(a, r, g, b));

    private static Brush Frozen(Color c)
    {
        var brush = new SolidColorBrush(c);
        brush.Freeze();
        return brush;
    }

    /// <summary>Turns blurring of whatever is behind the window on or off.</summary>
    private void SetBlur(bool on)
    {
        var accent = new AccentPolicy { AccentState = on ? ACCENT_ENABLE_BLURBEHIND : 0 };
        int size = Marshal.SizeOf(accent);
        IntPtr ptr = Marshal.AllocHGlobal(size);
        try
        {
            Marshal.StructureToPtr(accent, ptr, false);
            var data = new WindowCompositionAttributeData { Attribute = WCA_ACCENT_POLICY, Data = ptr, SizeOfData = size };
            SetWindowCompositionAttribute(_hwnd, ref data);
        }
        finally
        {
            Marshal.FreeHGlobal(ptr);
        }
    }

    // ================= Settings =================

    public void ApplyConfig(AppConfig config)
    {
        _config = config;
        _previewLayout = config.IsPreviewMode;

        try
        {
            var color = (Color)ColorConverter.ConvertFromString(config.AccentColor);
            Accent = Frozen(color);
            AccentSoft = Frozen(Color.FromArgb(0x55, color.R, color.G, color.B));
        }
        catch
        {
            // invalid colour → keep the current one
        }

        // Background: "clear" leaves the screen untouched; "blur"/"wallpaper" blur and dim it
        bool clear = !config.IsBackground("blur") && !config.IsBackground("wallpaper");
        SetBlur(config.IsBackground("blur"));
        Dim.Fill = clear
            ? Brushes.Transparent
            : Frozen((byte)Math.Round(Math.Clamp(config.BackgroundDim, 0, 1) * 255), 0, 0, 0);
        _hoverTimer.Interval = TimeSpan.FromMilliseconds(Math.Max(0, config.HoverPreviewDelayMs));
        UpdateCardPlacement();
        RegisterConfiguredHotkey();
        _pomodoro.Configure(config.PomodoroFocusMinutes, config.PomodoroBreakMinutes,
                            config.PomodoroLongBreakMinutes, config.PomodoroRoundsBeforeLongBreak);
        if (IsVisible && !_closing) BuildView();
    }

    /// <summary>Settings window: switch the hotkey off while you're typing a new one.</summary>
    public void PauseHotkey(bool pause)
    {
        _hotkeyPaused = pause;
        if (pause)
        {
            UnregisterHotKey(_hwnd, HotkeyId);
            _registeredHotkey = null;
        }
        else
        {
            RegisterConfiguredHotkey();
        }
    }

    private void RegisterConfiguredHotkey()
    {
        if (_hotkeyPaused || _registeredHotkey == _config.Hotkey) return; // nothing changed
        _registeredHotkey = _config.Hotkey;
        UnregisterHotKey(_hwnd, HotkeyId);

        if (_config.TryParseHotkey(out uint mods, out uint vk))
        {
            HotkeyText = _config.Hotkey;
        }
        else
        {
            MessageBox.Show($"Couldn't understand the hotkey \"{_config.Hotkey}\" in Settings — using Alt+` instead.", "WinOverview");
            HotkeyText = "Alt+`";
            mods = MOD_ALT;
            vk = VK_OEM_3;
        }

        if (!RegisterHotKey(_hwnd, HotkeyId, mods | MOD_NOREPEAT, vk))
            MessageBox.Show($"Couldn't register {HotkeyText} — another app (or Windows) is using it.\n" +
                            "Pick another one in Settings (right-click the tray icon).", "WinOverview");
    }

    private void UpdateCardPlacement()
    {
        bool center = string.Equals(_config.PanelPosition, "center", StringComparison.OrdinalIgnoreCase);
        Card.VerticalAlignment = center ? VerticalAlignment.Center : VerticalAlignment.Top;
        Card.RenderTransformOrigin = center ? new Point(0.5, 0.5) : new Point(0.5, 0);
        Card.Margin = center
            ? new Thickness(0)
            : new Thickness(0, ActualHeight * Math.Clamp(_config.TopMarginPercent, 0, 80) / 100, 0, 0);
    }

    private void OnSizeChanged(object sender, SizeChangedEventArgs e) => UpdateCardPlacement();

    // ================= Hotkey / show / hide =================

    /// <summary>How long the last open took, from the key press to the first frame on screen (ms).</summary>
    public double? LastOpenMs { get; private set; }
    private readonly System.Diagnostics.Stopwatch _openClock = new();

    private IntPtr WndProc(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled)
    {
        if (msg == WM_HOTKEY && wParam.ToInt32() == HotkeyId)
        {
            if (!_closing)
            {
                if (IsVisible) HideOverview();
                else
                {
                    _openClock.Restart();
                    ShowOverview();
                    CompositionTarget.Rendering += OnFirstFrame;
                }
            }
            handled = true;
        }
        return IntPtr.Zero;
    }

    private void OnFirstFrame(object? sender, EventArgs e)
    {
        CompositionTarget.Rendering -= OnFirstFrame;
        LastOpenMs = _openClock.Elapsed.TotalMilliseconds;
    }

    /// <summary>After an unexpected error: get back to a clean, hidden state.</summary>
    public void ResetAfterError()
    {
        try
        {
            _closing = false;
            _pressed = null;
            _dragging = false;
            if (IsVisible) HideNow();
        }
        catch
        {
            // already as clean as it gets
        }
    }

    /// <summary>Tray icon click / menu. Waits a moment so the tray menu can close first.</summary>
    public async void OpenFromTray()
    {
        await Task.Delay(150);
        if (!IsVisible && !_closing) ShowOverview();
    }

    private void ShowOverview()
    {
        _previousForeground = GetForegroundWindow();

        // Which monitor, and how much of it (everything except the taskbar)
        var monitor = _config.OpenOnMouseMonitor ? MonitorHelper.AtCursor() : MonitorHelper.Primary();
        _monitorPx = monitor.Bounds;
        _screenAspect = Math.Clamp((double)_monitorPx.Height / Math.Max(1, _monitorPx.Width), 0.3, 1.2);
        _openAnimDone = false;
        RECT area = monitor.Work;
        if (monitor.IsPrimary)
        {
            TaskbarHelper.Show(_config.ShowAutoHiddenTaskbar); // keeps it on top (and un-hides it if you chose that)
            area = MinusTaskbar(monitor.Bounds);
        }
        // With auto-hide, snapped windows should use the full screen (the taskbar hides again later)
        _snapAreaPx = monitor.IsPrimary && TaskbarHelper.TemporarilyShown ? monitor.Bounds : monitor.Work;

        Wallpaper.Source = _config.IsBackground("wallpaper") ? WallpaperHelper.GetBlurred() : null;
        ResetKeyboardState();
        BuildView();
        AnimateIn();
        _widgetTimer.Start();

        SetWindowPos(_hwnd, HWND_TOPMOST, area.Left, area.Top, area.Width, area.Height, SWP_NOACTIVATE);
        Show();
        SetWindowPos(_hwnd, HWND_TOPMOST, area.Left, area.Top, area.Width, area.Height, SWP_NOACTIVATE);

        if (!_pinned) _pinned = VirtualDesktop.PinWindow(_hwnd) >= 0; // open on any desktop

        Activate();
        Focus();
    }

    /// <summary>The monitor minus the main taskbar, in physical pixels.</summary>
    private static RECT MinusTaskbar(RECT bounds)
    {
        var (tb, edge) = TaskbarHelper.GetPosition();
        var r = bounds;
        if (tb.Width <= 0 || tb.Height <= 0) return r;

        switch (edge)
        {
            case ABE_BOTTOM: r.Bottom = Math.Min(r.Bottom, tb.Top); break;
            case ABE_TOP: r.Top = Math.Max(r.Top, tb.Bottom); break;
            case ABE_LEFT: r.Left = Math.Max(r.Left, tb.Right); break;
            case ABE_RIGHT: r.Right = Math.Min(r.Right, tb.Left); break;
        }
        if (r.Bottom - r.Top < 100 || r.Right - r.Left < 100) return bounds; // sanity check
        return r;
    }

    /// <summary>Fade out, run <paramref name="action"/> (while still focused), then hide.</summary>
    private void HideOverview(Action? action = null, bool animate = true)
    {
        if (!IsVisible || _closing) return;
        _closing = true;
        _thumbHost?.Clear();   // live previews can't fade, so they go first
        CancelDrag();
        HidePreview();

        if (!animate)
        {
            RemoveFlyout();
            action?.Invoke();
            _closing = false;
            HideNow();
            return;
        }

        CloseFlyout();
        AnimateOut(() =>
        {
            action?.Invoke();
            _closing = false;
            HideNow();
        });
    }

    private void HideNow()
    {
        _widgetTimer.Stop();
        Hide();
        TaskbarHelper.Restore(); // back to auto-hide, if that's how you had it

        // While hidden, keep nothing heavy around: it's all rebuilt the next time you open it
        TileGrid.Children.Clear();
        WidgetRow.Children.Clear();
        FlyoutLayer.Children.Clear();
        DragLayer.Children.Clear();
        Wallpaper.Source = null;
        _tiles.Clear();
        _previewItems.Clear();
        ResetKeyboardState();
        _thumbHost?.Dispose();
        _thumbHost = null;
        _windows = new();
        _pomoTime = _musicTitle = null;
        _vinylSpin?.Stop();
        _vinyl = null;
        _vinylSpin = null;
        _vinylSpinning = false;

        // Once things settle, free that memory and give it back to Windows
        Dispatcher.BeginInvoke(DispatcherPriority.ApplicationIdle, TrimMemory);
    }

    private static void TrimMemory()
    {
        GC.Collect(GC.MaxGeneration, GCCollectionMode.Forced, blocking: true, compacting: true);
        SetProcessWorkingSetSize(GetCurrentProcess(), -1, -1);
    }

    private static CubicEase EaseOut => new() { EasingMode = EasingMode.EaseOut };
    private static CubicEase EaseInOut => new() { EasingMode = EasingMode.EaseInOut };

    private void AnimateIn()
    {
        var d = TimeSpan.FromMilliseconds(OpenMs);
        Backdrop.BeginAnimation(OpacityProperty, new DoubleAnimation(0, 1, d) { EasingFunction = EaseOut });
        var fade = new DoubleAnimation(0, 1, d) { EasingFunction = EaseOut };
        fade.Completed += (_, _) =>
        {
            _openAnimDone = true;
            ShowThumbnails(); // live previews appear once the panel has settled
        };
        Card.BeginAnimation(OpacityProperty, fade);
        var zoom = new DoubleAnimation(0.94, 1, d) { EasingFunction = EaseOut };
        _cardScale.BeginAnimation(ScaleTransform.ScaleXProperty, zoom);
        _cardScale.BeginAnimation(ScaleTransform.ScaleYProperty, zoom);
    }

    private void AnimateOut(Action done)
    {
        var d = TimeSpan.FromMilliseconds(CloseMs);
        var fade = new DoubleAnimation(0, d) { EasingFunction = EaseOut };
        fade.Completed += (_, _) => done();
        Backdrop.BeginAnimation(OpacityProperty, new DoubleAnimation(0, d));
        Card.BeginAnimation(OpacityProperty, fade);
        var zoom = new DoubleAnimation(0.97, d) { EasingFunction = EaseOut };
        _cardScale.BeginAnimation(ScaleTransform.ScaleXProperty, zoom);
        _cardScale.BeginAnimation(ScaleTransform.ScaleYProperty, zoom);
    }

    private async void Refresh(int delayMs = 60)
    {
        await Task.Delay(delayMs); // let Windows finish moving/closing
        if (IsVisible && !_closing) BuildView();
    }

    private void OnDeactivated(object? sender, EventArgs e) => HideOverview(animate: false);

    /// <summary>Clicking outside the snap picker closes it; clicking outside the panel closes the overview.</summary>
    private void OnBackdropClick(object sender, MouseButtonEventArgs e)
    {
        if (_flyout != null)
        {
            if (!_flyout.IsMouseOver) CloseFlyout();
            return;
        }
        if (!Card.IsMouseOver) HideOverview();
    }

    protected override void OnClosed(EventArgs e)
    {
        if (_hwnd != IntPtr.Zero) UnregisterHotKey(_hwnd, HotkeyId);
        TaskbarHelper.Restore();
        base.OnClosed(e);

        // Closing the window (e.g. "taskkill") quits the whole app, tray icon included
        QuitApp();
    }

    private static void QuitApp() => (Application.Current as App)?.Quit();
}
