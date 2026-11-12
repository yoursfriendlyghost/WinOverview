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

// Desktop tiles, app icons and live-preview items.
public partial class MainWindow
{
    // ================= Build the desktop row =================

    private void BuildView()
    {
        HidePreview();
        _hovered = null;
        _thumbHost?.Clear();
        _previewItems.Clear();
        IntPtr keepSelected = _selected?.Info.Handle ?? IntPtr.Zero;
        _navItems.Clear();
        _selected = null;
        TileGrid.Children.Clear();
        _tiles.Clear();

        int count = Math.Max(1, VirtualDesktop.GetDesktopCount());
        _current = VirtualDesktop.GetCurrentDesktopNumber();
        _windows = WindowEnumerator.GetWindows(_hwnd, _current); // most recently used first
        IconHelper.Prune(_windows.Select(w => w.Handle));


        for (int i = 0; i < count; i++)
            MakeDesktopTile(i, _windows.Where(w => w.Desktop == i).ToList(), canClose: count > 1);

        MakeNewDesktopTile();
        _justMoved = IntPtr.Zero;
        BuildWidgets();
        RestoreKeyboardState(keepSelected);

        // Live previews are placed once WPF has laid the tiles out
        if (_previewLayout) Dispatcher.BeginInvoke(DispatcherPriority.Loaded, ShowThumbnails);
    }

    /// <summary>Adds a tile to the current row, starting a new row after "desktops per row" tiles.</summary>
    private void AddTile(Border tile)
    {
        int perRow = Math.Clamp(_config.DesktopsPerRow, 1, 20);
        if (TileGrid.Children.Count == 0 || ((StackPanel)TileGrid.Children[^1]).Children.Count >= perRow)
            TileGrid.Children.Add(new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Center });

        tile.VerticalAlignment = VerticalAlignment.Top; // tiles in a row can have different heights
        ((StackPanel)TileGrid.Children[^1]).Children.Add(tile);
    }

    private static Border MakeCloseButton(double size, string tooltip, Action onClick)
    {
        var button = new Border
        {
            Width = size,
            Height = size,
            CornerRadius = new CornerRadius(size / 2),
            Background = CloseBg,
            Cursor = Cursors.Hand,
            ToolTip = tooltip,
            Visibility = Visibility.Collapsed,
            Child = new TextBlock
            {
                Text = "✕",
                FontSize = size * 0.5,
                Foreground = Brushes.White,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            },
        };
        button.MouseEnter += (_, _) => button.Background = CloseHoverBg;
        button.MouseLeave += (_, _) => button.Background = CloseBg;
        button.MouseLeftButtonDown += (_, e) => e.Handled = true; // don't start a drag / switch desktop
        button.MouseLeftButtonUp += (_, e) =>
        {
            e.Handled = true;
            onClick();
        };
        return button;
    }

    private static Border MakePill(int index, bool current) => new()
    {
        Height = PillHeight,
        MinWidth = 28,
        Padding = new Thickness(8, 0, 8, 0),
        CornerRadius = new CornerRadius(PillHeight / 2),
        Background = current ? Accent : PillBg,
        HorizontalAlignment = HorizontalAlignment.Left,
        Child = new TextBlock
        {
            Text = (index + 1).ToString(),
            FontSize = 11,
            FontWeight = FontWeights.SemiBold,
            Foreground = current ? AccentText : PillText,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        },
    };

    private void MakeDesktopTile(int index, List<WindowInfo> windows, bool canClose)
    {
        var tile = new DesktopTile { Index = index, IsCurrent = index == _current };

        int capacity = GridCols * GridRows;
        int shown = windows.Count > capacity ? capacity - 1 : windows.Count;
        int navStart = _navItems.Count;   // this desktop's apps get added from here
        int navCols = 1;

        double bodyW = BodyW, bodyH = BodyH;
        FrameworkElement body;
        if (_previewLayout)
        {
            (bodyW, bodyH) = PreviewGridSize(windows.Count);
            body = MakePreviewGrid(windows, bodyW, bodyH);
            navCols = PreviewGridShape(windows.Count).Cols;
        }
        else if (windows.Count == 0)
        {
            body = new TextBlock
            {
                Text = "Empty",
                FontSize = 12,
                Foreground = FaintText,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            };
        }
        else
        {
            int cells = shown + (windows.Count > shown ? 1 : 0);
            var grid = new UniformGrid
            {
                Columns = Math.Min(cells <= 4 ? 2 : GridCols, cells),
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            };
            foreach (var w in windows.Take(shown)) grid.Children.Add(MakeIcon(w));
            if (windows.Count > shown) grid.Children.Add(MakeMoreCell(windows.Count - shown));
            body = grid;
            navCols = grid.Columns;
        }

        // Where each app sits in this desktop's grid (for ↑ ↓)
        for (int i = navStart; i < _navItems.Count; i++)
        {
            _navItems[i].IndexInDesktop = i - navStart;
            _navItems[i].DesktopCols = Math.Max(1, navCols);
        }

        // Top row: number pill on the left, close-desktop button on the right
        var pill = MakePill(index, tile.IsCurrent);
        var header = new Grid { Height = PillHeight, Margin = new Thickness(0, 0, 0, 10), Children = { pill } };
        Border? closeDesktop = null;
        if (canClose)
        {
            closeDesktop = MakeCloseButton(PillHeight, "Close desktop (apps move to desktop 1)", () => CloseDesktop(index));
            closeDesktop.HorizontalAlignment = HorizontalAlignment.Right;
            header.Children.Add(closeDesktop);
        }

        var layout = new DockPanel
        {
            Width = bodyW,
            Height = PillHeight + 10 + bodyH,
            Background = Brushes.Transparent,
            LastChildFill = true,
        };
        DockPanel.SetDock(header, Dock.Top);
        layout.Children.Add(header);
        layout.Children.Add(body);

        tile.Root = new Border
        {
            Child = layout,
            Margin = new Thickness(5),
            Padding = new Thickness(TilePad - 1),
            CornerRadius = new CornerRadius(14),
            Background = TileBg,
            BorderThickness = new Thickness(1),
            BorderBrush = Brushes.Transparent,
            Cursor = Cursors.Hand,
        };

        tile.Root.MouseEnter += (_, _) =>
        {
            if (_dragging) return;
            tile.Root.Background = TileHoverBg;
            if (closeDesktop != null) closeDesktop.Visibility = Visibility.Visible;
        };
        tile.Root.MouseLeave += (_, _) =>
        {
            if (closeDesktop != null) closeDesktop.Visibility = Visibility.Collapsed;
            if (!_dragging) tile.Root.Background = TileBg;
        };
        layout.MouseLeftButtonUp += (_, _) => { if (_pressed == null) SwitchTo(index); };

        _tiles.Add(tile);
        AddTile(tile.Root);
    }

    private void MakeNewDesktopTile()
    {
        var tile = new DesktopTile { Index = -1 };

        var content = new Grid
        {
            Width = TileW - 2 * TilePad,
            Height = TileH - 2 * TilePad,
            Background = Brushes.Transparent,
            Children =
            {
                new TextBlock
                {
                    Text = "+",
                    FontSize = 28,
                    FontWeight = FontWeights.Light,
                    Foreground = FaintText,
                    HorizontalAlignment = HorizontalAlignment.Center,
                    VerticalAlignment = VerticalAlignment.Center,
                },
            },
        };

        tile.Root = new Border
        {
            Child = content,
            Margin = new Thickness(5),
            Padding = new Thickness(TilePad - 1),
            CornerRadius = new CornerRadius(14),
            Background = Brushes.Transparent,
            BorderThickness = new Thickness(1),
            BorderBrush = NewTileBorder,
            Cursor = Cursors.Hand,
            ToolTip = "New desktop (click, or drop an app here)",
        };

        tile.Root.MouseEnter += (_, _) => { if (!_dragging) tile.Root.Background = TileBg; };
        tile.Root.MouseLeave += (_, _) => { if (!_dragging) tile.Root.Background = Brushes.Transparent; };
        content.MouseLeftButtonUp += (_, _) =>
        {
            if (_pressed != null) return;
            int created = CreateDesktop();
            if (created >= 0) SwitchTo(created);
        };

        _tiles.Add(tile);
        AddTile(tile.Root);
    }

    // ================= App icons =================

    private static FrameworkElement MakeIconVisual(ImageSource? icon, string title, double size = IconSize)
    {
        if (icon != null)
        {
            var image = new Image { Source = icon, Width = size, Height = size };
            RenderOptions.SetBitmapScalingMode(image, BitmapScalingMode.HighQuality);
            return image;
        }

        return new Border
        {
            Width = size,
            Height = size,
            CornerRadius = new CornerRadius(size * 0.28),
            Background = LetterBg,
            Child = new TextBlock
            {
                Text = title.Length > 0 ? char.ToUpper(title[0]).ToString() : "?",
                Foreground = Brushes.White,
                FontSize = size * 0.5,
                FontWeight = FontWeights.SemiBold,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            },
        };
    }

    private static Border MakeCell(UIElement child) => new()
    {
        Width = Cell,
        Height = Cell,
        Margin = new Thickness(Gap / 2),
        CornerRadius = new CornerRadius(10),
        Background = CellBg,
        BorderThickness = new Thickness(1.5),
        BorderBrush = Brushes.Transparent,
        Child = child,
    };

    private Border MakeIcon(WindowInfo w)
    {
        var item = new IconItem { Info = w, Icon = IconHelper.GetIcon(w.Handle) };

        var visual = MakeIconVisual(item.Icon, w.Title);
        if (w.Minimized) visual.Opacity = 0.45;

        // × sits inside the square's top-right corner
        var close = MakeCloseButton(16, "Close", () => CloseWindow(w));
        close.HorizontalAlignment = HorizontalAlignment.Right;
        close.VerticalAlignment = VerticalAlignment.Top;
        close.Margin = new Thickness(0, 1, 1, 0);

        var cell = MakeCell(new Grid { Children = { visual, close } });
        // With hover previews on, the preview replaces the tooltip
        bool hoverPreview = _config.HoverPreview && !_previewLayout;
        if (!hoverPreview || w.Minimized) cell.ToolTip = w.Minimized ? w.Title + " (minimized)" : w.Title;
        cell.Cursor = Cursors.Hand;
        item.Cell = cell;

        if (w.Handle == _justMoved) PopIn(cell);
        WireItem(item, close);
        _navItems.Add(item);
        return cell;
    }

    /// <summary>"previews" mode: a live mini-window at the app's real position on the mini-screen.</summary>
    private Border MakePreviewItem(WindowInfo w, Rect local, RECT source, bool live)
    {
        var item = new IconItem { Info = w, Icon = IconHelper.GetIcon(w.Handle), IsPreview = live, Source = source };

        // The app's icon: what minimized apps show, and what live ones show while previews step aside (dragging)
        double iconSize = Math.Clamp(Math.Min(local.Width, local.Height) * 0.5, 12, 28);
        var icon = MakeIconVisual(item.Icon, w.Title, iconSize);
        icon.HorizontalAlignment = HorizontalAlignment.Center;
        icon.VerticalAlignment = VerticalAlignment.Center;

        // × in the top-right corner (the preview leaves that corner uncovered on hover)
        var close = MakeCloseButton(14, "Close", () => CloseWindow(w));
        close.HorizontalAlignment = HorizontalAlignment.Right;
        close.VerticalAlignment = VerticalAlignment.Top;

        // Slightly bigger than the preview, so its border shows as an outline around it
        var cell = new Border
        {
            Width = local.Width + 2 * Outline,
            Height = local.Height + 2 * Outline,
            BorderThickness = new Thickness(Outline),
            BorderBrush = Brushes.Transparent,
            Background = CellBg,
            CornerRadius = new CornerRadius(3),
            Child = new Grid { Children = { icon, close } },
            Cursor = Cursors.Hand,
            ToolTip = w.Minimized ? w.Title + " (minimized)" : w.Title,
        };
        if (w.Minimized) icon.Opacity = 0.5;
        Canvas.SetLeft(cell, local.X - Outline);
        Canvas.SetTop(cell, local.Y - Outline);
        item.Cell = cell;

        WireItem(item, close);
        _navItems.Add(item);
        if (live) _previewItems.Add(item);
        return cell;
    }

    /// <summary>"previews" mode: one desktop's apps as a grid of live thumbnails (most recent first).</summary>
    /// <summary>Columns, rows and how many apps get a slot (the rest are summed up as "+N").</summary>
    private static (int Cols, int Rows, int Shown, int Slots) PreviewGridShape(int count)
    {
        const int maxSlots = PreviewMaxCols * PreviewMaxRows;
        int shown = count > maxSlots ? maxSlots - 1 : count;
        int slots = Math.Max(1, shown + (count > shown ? 1 : 0));
        int cols = slots <= 1 ? 1 : slots <= 4 ? 2 : 3;
        int rows = (int)Math.Ceiling(slots / (double)cols);
        return (cols, rows, shown, slots);
    }

    /// <summary>A desktop's size in "previews" view: bigger the more apps it has.</summary>
    private static (double W, double H) PreviewGridSize(int count)
    {
        var (cols, rows, _, _) = PreviewGridShape(count);
        return (cols * PreviewSlotW + (cols - 1) * PreviewGap, rows * PreviewSlotH + (rows - 1) * PreviewGap);
    }

    private FrameworkElement MakePreviewGrid(List<WindowInfo> windows, double bodyW, double bodyH)
    {
        var area = new Grid { Width = bodyW, Height = bodyH };
        if (windows.Count == 0)
        {
            area.Children.Add(new TextBlock
            {
                Text = "Empty",
                FontSize = 12,
                Foreground = FaintText,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            });
            return area;
        }

        // Up to 9 apps (3 × 3); if there are more, the last slot says "+N"
        var (cols, rows, shown, slots) = PreviewGridShape(windows.Count);
        const double gap = PreviewGap;
        double slotW = (bodyW - (cols - 1) * gap) / cols;
        double slotH = (bodyH - (rows - 1) * gap) / rows;

        var canvas = new Canvas { Width = bodyW, Height = bodyH, Background = Brushes.Transparent };
        for (int i = 0; i < slots; i++)
        {
            double slotX = (i % cols) * (slotW + gap);
            double slotY = (i / cols) * (slotH + gap);

            if (i >= shown) // "+N more"
            {
                var more = MakeMoreCell(windows.Count - shown);
                more.Margin = new Thickness(0);
                double size = Math.Min(Math.Min(slotW, slotH), Cell);
                more.Width = more.Height = size;
                Canvas.SetLeft(more, slotX + (slotW - size) / 2);
                Canvas.SetTop(more, slotY + (slotH - size) / 2);
                canvas.Children.Add(more);
                continue;
            }

            var w = windows[i];
            bool hasFrame = FrameOf(w.Handle, out RECT frame, out RECT source);
            bool live = !w.Minimized && hasFrame;

            // Keep the window's shape inside its slot (minimized apps get a 16:9 box)
            double aspect = live ? (double)frame.Width / Math.Max(1, frame.Height) : 16.0 / 9;
            double width = slotW - 2 * Outline, height = width / aspect;
            if (height > slotH - 2 * Outline)
            {
                height = slotH - 2 * Outline;
                width = height * aspect;
            }
            var local = new Rect(slotX + (slotW - width) / 2, slotY + (slotH - height) / 2, width, height);
            canvas.Children.Add(MakePreviewItem(w, local, live ? source : default, live));
        }
        area.Children.Add(canvas);
        return area;
    }

    /// <summary>A window's visible frame (no invisible resize borders), and that frame relative to the window.</summary>
    private static bool FrameOf(IntPtr h, out RECT frame, out RECT source)
    {
        frame = source = default;
        if (!GetWindowRect(h, out RECT wr)) return false;

        frame = wr;
        if (DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, out RECT f, Marshal.SizeOf<RECT>()) == 0
            && f.Width > 0 && f.Height > 0) frame = f;
        if (frame.Width <= 0 || frame.Height <= 0) return false;

        source = new RECT(frame.Left - wr.Left, frame.Top - wr.Top, frame.Right - wr.Left, frame.Bottom - wr.Top);
        return true;
    }

    /// <summary>Hover, click, drag and close handling — the same for icons and live previews.</summary>
    private void WireItem(IconItem item, Border close)
    {
        var cell = item.Cell;
        var w = item.Info;

        cell.MouseEnter += (_, _) =>
        {
            if (_pressed != null) return;
            _hovered = item;
            Select(item);          // the mouse and the keyboard share one selection
            PaintItem(item);
            close.Visibility = Visibility.Visible;
            if (item.IsPreview) _thumbHost?.SetCutout(w.Handle);

            if (_config.HoverPreview && !_previewLayout && _flyout == null)
            {
                _hoverTimer.Stop();
                _hoverTimer.Start();
            }
        };
        cell.MouseLeave += (_, _) =>
        {
            close.Visibility = Visibility.Collapsed;
            if (item.IsPreview) _thumbHost?.SetCutout(IntPtr.Zero);
            if (_hovered == item)
            {
                _hovered = null;
                _hoverTimer.Stop();
                HidePreview();
            }
            if (_pressed == item) return;
            PaintItem(item);
        };

        cell.MouseLeftButtonDown += (_, e) =>
        {
            e.Handled = true;
            if (_closing) return;
            _pressed = item;
            _pressPoint = e.GetPosition(Root);
            _dragging = false;
            cell.CaptureMouse();
        };
        cell.MouseMove += (_, e) => OnIconMouseMove(item, e.GetPosition(Root));
        cell.MouseLeftButtonUp += (_, e) =>
        {
            e.Handled = true;
            OnIconMouseUp(item, e.GetPosition(Root));
            cell.ReleaseMouseCapture();
        };
        cell.LostMouseCapture += (_, _) => { if (_pressed == item) CancelDrag(); };
        cell.MouseUp += (_, e) =>
        {
            if (e.ChangedButton != MouseButton.Middle || _closing) return;
            e.Handled = true;
            CloseWindow(w);
        };
    }


    private static Border MakeMoreCell(int extra) => MakeCell(new TextBlock
    {
        Text = "+" + extra,
        Foreground = PillText,
        FontSize = 13,
        FontWeight = FontWeights.SemiBold,
        HorizontalAlignment = HorizontalAlignment.Center,
        VerticalAlignment = VerticalAlignment.Center,
    });

    /// <summary>The app that just moved here pops into its new spot.</summary>
    private static void PopIn(FrameworkElement element)
    {
        var scale = new ScaleTransform(0.4, 0.4);
        element.RenderTransformOrigin = new Point(0.5, 0.5);
        element.RenderTransform = scale;
        element.Opacity = 0;
        var d = TimeSpan.FromMilliseconds(320);
        var spring = new BackEase { EasingMode = EasingMode.EaseOut, Amplitude = 0.5 };
        element.BeginAnimation(OpacityProperty, new DoubleAnimation(1, TimeSpan.FromMilliseconds(150)));
        scale.BeginAnimation(ScaleTransform.ScaleXProperty, new DoubleAnimation(1, d) { EasingFunction = spring });
        scale.BeginAnimation(ScaleTransform.ScaleYProperty, new DoubleAnimation(1, d) { EasingFunction = spring });
    }
}
