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

// The snap-layout picker shown when an app is dropped outside the panel.
public partial class MainWindow
{
    // ================= Snap picker =================

    /// <summary>The app you were using before opening the overview — it fills the other half of the split.</summary>
    private WindowInfo? PartnerFor(WindowInfo dragged) =>
        _windows.FirstOrDefault(w => w.Handle == _previousForeground && w.Handle != dragged.Handle);

    private void ShowSnapFlyout(IconItem item, Point at)
    {
        RemoveFlyout();
        HidePreview();
        _thumbHost?.Suspend(); // the picker may overlap the live previews
        var w = item.Info;
        var partner = PartnerFor(w);
        var partnerIcon = partner != null ? IconHelper.GetIcon(partner.Handle) : null;

        // ---- Header: "<App> + <previous app>" ----
        var header = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(6, 2, 6, 10) };
        header.Children.Add(MakeIconVisual(item.Icon, w.Title, 22));
        header.Children.Add(new TextBlock
        {
            Text = ShortTitle(w.Title),
            Foreground = Brushes.White,
            FontSize = 13,
            FontWeight = FontWeights.SemiBold,
            Margin = new Thickness(8, 0, 0, 0),
            VerticalAlignment = VerticalAlignment.Center,
        });
        if (partner != null)
        {
            header.Children.Add(new TextBlock
            {
                Text = "  +  ",
                Foreground = FaintText,
                FontSize = 13,
                VerticalAlignment = VerticalAlignment.Center,
            });
            header.Children.Add(MakeIconVisual(partnerIcon, partner.Title, 22));
            header.Children.Add(new TextBlock
            {
                Text = ShortTitle(partner.Title),
                Foreground = PillText,
                FontSize = 13,
                Margin = new Thickness(8, 0, 0, 0),
                VerticalAlignment = VerticalAlignment.Center,
            });
        }

        var subtitle = new TextBlock
        {
            Text = partner != null ? "Pick a spot — the other app fills the rest" : "Pick a spot",
            Foreground = FaintText,
            FontSize = 11,
            Margin = new Thickness(6, -6, 6, 8),
        };

        // ---- Layout previews ----
        var grid = new UniformGrid { Columns = 3 };
        int i = 0;
        foreach (var layout in SnapLayouts)
        {
            var preview = MakeLayoutPreview(w, item.Icon, partner, partnerIcon, layout);
            grid.Children.Add(preview);
            Rise(preview, delayMs: 60 + i * 22); // staggered entrance
            i++;
        }

        var flyout = new Border
        {
            Background = new LinearGradientBrush(
                Color.FromArgb(0xF5, 0x24, 0x24, 0x30), Color.FromArgb(0xF5, 0x18, 0x18, 0x20), 90),
            BorderBrush = AccentSoft,
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(18),
            Padding = new Thickness(10),
            Child = new StackPanel { Children = { header, subtitle, grid } },
            Effect = new DropShadowEffect { BlurRadius = 50, ShadowDepth = 12, Opacity = 0.55, Color = Colors.Black },
        };

        // Place it at the drop point, kept on screen
        flyout.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        var size = flyout.DesiredSize;
        double x = Math.Clamp(at.X - size.Width / 2, 12, Math.Max(12, ActualWidth - size.Width - 12));
        double y = Math.Clamp(at.Y - 30, 12, Math.Max(12, ActualHeight - size.Height - 12));
        Canvas.SetLeft(flyout, x);
        Canvas.SetTop(flyout, y);

        // Spring in from where you dropped the app
        var scale = new ScaleTransform(0.8, 0.8);
        var slide = new TranslateTransform(0, 18);
        flyout.RenderTransformOrigin = new Point(Math.Clamp((at.X - x) / size.Width, 0, 1), 0);
        flyout.RenderTransform = new TransformGroup { Children = { scale, slide } };
        flyout.Opacity = 0;

        FlyoutLayer.Children.Add(flyout);
        _flyout = flyout;

        var d = TimeSpan.FromMilliseconds(320);
        var spring = new BackEase { EasingMode = EasingMode.EaseOut, Amplitude = 0.35 };
        flyout.BeginAnimation(OpacityProperty, new DoubleAnimation(1, TimeSpan.FromMilliseconds(160)));
        scale.BeginAnimation(ScaleTransform.ScaleXProperty, new DoubleAnimation(1, d) { EasingFunction = spring });
        scale.BeginAnimation(ScaleTransform.ScaleYProperty, new DoubleAnimation(1, d) { EasingFunction = spring });
        slide.BeginAnimation(TranslateTransform.YProperty, new DoubleAnimation(0, d) { EasingFunction = EaseOut });
    }

    private static string ShortTitle(string title)
    {
        // "Document - App Name" → "App Name"
        int dash = title.LastIndexOf(" - ", StringComparison.Ordinal);
        string s = dash > 0 ? title[(dash + 3)..] : title;
        return s.Length > 22 ? s[..21] + "…" : s;
    }

    /// <summary>Fade + rise into place after a delay.</summary>
    private static void Rise(FrameworkElement element, double delayMs)
    {
        var rise = new TranslateTransform(0, 10);
        element.RenderTransform = rise;
        element.Opacity = 0;
        var begin = TimeSpan.FromMilliseconds(delayMs);
        var d = TimeSpan.FromMilliseconds(220);
        element.BeginAnimation(OpacityProperty, new DoubleAnimation(1, d) { BeginTime = begin });
        rise.BeginAnimation(TranslateTransform.YProperty,
            new DoubleAnimation(0, d) { BeginTime = begin, EasingFunction = EaseOut });
    }

    /// <summary>
    /// One mini screen with clickable zones. Hovering a zone shows where each app will go:
    /// the dragged app in that zone, the previous app in the next one.
    /// </summary>
    private FrameworkElement MakeLayoutPreview(WindowInfo w, ImageSource? icon,
                                               WindowInfo? partner, ImageSource? partnerIcon, Rect[] zones)
    {
        const double gap = 2;
        var canvas = new Canvas { Width = PreviewW, Height = PreviewH };
        var zoneBorders = new List<Border>();

        var card = new Border
        {
            Child = canvas,
            Margin = new Thickness(4),
            Padding = new Thickness(5),
            CornerRadius = new CornerRadius(10),
            Background = PreviewBg,
            BorderThickness = new Thickness(1),
            BorderBrush = Brushes.Transparent,
        };

        void ResetZones()
        {
            foreach (var zb in zoneBorders)
            {
                zb.Background = ZoneBg;
                zb.Child = null;
            }
        }

        for (int zi = 0; zi < zones.Length; zi++)
        {
            var z = zones[zi];
            int index = zi;
            var zone = new Border
            {
                Width = z.Width * PreviewW - 2 * gap,
                Height = z.Height * PreviewH - 2 * gap,
                CornerRadius = new CornerRadius(5),
                Background = ZoneBg,
                Cursor = Cursors.Hand,
            };
            Canvas.SetLeft(zone, z.X * PreviewW + gap);
            Canvas.SetTop(zone, z.Y * PreviewH + gap);
            zoneBorders.Add(zone);

            zone.MouseEnter += (_, _) =>
            {
                ResetZones();
                zone.Background = Accent;
                zone.Child = MakeZoneIcon(icon, w.Title);
                int other = PartnerZone(zones.Length, index);
                if (partner != null && other >= 0)
                {
                    zoneBorders[other].Background = AccentSoft;
                    zoneBorders[other].Child = MakeZoneIcon(partnerIcon, partner.Title);
                }
            };
            zone.MouseLeave += (_, _) => ResetZones();
            zone.MouseLeftButtonUp += (_, e) =>
            {
                e.Handled = true;
                Flash(zone);
                SnapTo(w, zones, index, partner);
            };
            canvas.Children.Add(zone);
        }

        card.MouseEnter += (_, _) => { card.Background = PreviewHoverBg; card.BorderBrush = AccentSoft; };
        card.MouseLeave += (_, _) => { card.Background = PreviewBg; card.BorderBrush = Brushes.Transparent; ResetZones(); };
        return card;
    }

    private static FrameworkElement MakeZoneIcon(ImageSource? icon, string title)
    {
        var v = MakeIconVisual(icon, title, 16);
        v.HorizontalAlignment = HorizontalAlignment.Center;
        v.VerticalAlignment = VerticalAlignment.Center;
        return v;
    }

    /// <summary>Which zone the previous app takes: the first zone that isn't the chosen one.</summary>
    private static int PartnerZone(int zoneCount, int chosen) =>
        zoneCount < 2 ? -1 : chosen == 0 ? 1 : 0;

    private static void Flash(Border zone)
    {
        zone.Background = Brushes.White;
        zone.BeginAnimation(OpacityProperty, new DoubleAnimation(0.6, 1, TimeSpan.FromMilliseconds(120)));
    }

    /// <summary>Animate the picker away, then remove it.</summary>
    private void CloseFlyout()
    {
        if (_flyout == null) return;
        var flyout = _flyout;
        _flyout = null;
        ResumeThumbnails();

        var d = TimeSpan.FromMilliseconds(110);
        var fade = new DoubleAnimation(0, d);
        fade.Completed += (_, _) => FlyoutLayer.Children.Remove(flyout);
        flyout.BeginAnimation(OpacityProperty, fade);
        if (flyout.RenderTransform is TransformGroup g && g.Children[0] is ScaleTransform s)
        {
            s.BeginAnimation(ScaleTransform.ScaleXProperty, new DoubleAnimation(0.92, d));
            s.BeginAnimation(ScaleTransform.ScaleYProperty, new DoubleAnimation(0.92, d));
        }
    }

    private void RemoveFlyout()
    {
        FlyoutLayer.Children.Clear();
        _flyout = null;
        ResumeThumbnails();
    }

    /// <summary>
    /// Put the dragged app in the chosen zone and the previous app in the partner zone
    /// (a split screen), both on this desktop, then focus the dragged app.
    /// </summary>
    private void SnapTo(WindowInfo w, Rect[] zones, int chosen, WindowInfo? partner)
    {
        RECT area = _snapAreaPx;
        int other = PartnerZone(zones.Length, chosen);

        HideOverview(() =>
        {
            if (w.Desktop != _current) VirtualDesktop.MoveWindowToDesktopNumber(w.Handle, _current);

            PlaceWindow(w.Handle, zones[chosen], area);
            if (partner != null && other >= 0) PlaceWindow(partner.Handle, zones[other], area);

            SetForegroundWindow(w.Handle);
        });
    }

    /// <summary>Move/resize a window to a zone of <paramref name="area"/> (physical pixels).</summary>
    private static void PlaceWindow(IntPtr h, Rect zone, RECT area)
    {
        if (zone.Width >= 1 && zone.Height >= 1)
        {
            ShowWindow(h, SW_MAXIMIZE);
            return;
        }

        if (IsIconic(h) || IsZoomed(h)) ShowWindow(h, SW_RESTORE);

        int x = area.Left + (int)Math.Round(zone.X * area.Width);
        int y = area.Top + (int)Math.Round(zone.Y * area.Height);
        int cw = (int)Math.Round(zone.Width * area.Width);
        int ch = (int)Math.Round(zone.Height * area.Height);

        // Windows have invisible resize borders; grow the rect so the visible edges line up
        int l = 0, t = 0, r = 0, b = 0;
        if (GetWindowRect(h, out RECT wr)
            && DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, out RECT fr, Marshal.SizeOf<RECT>()) == 0)
        {
            l = fr.Left - wr.Left;
            t = fr.Top - wr.Top;
            r = wr.Right - fr.Right;
            b = wr.Bottom - fr.Bottom;
        }
        SetWindowPos(h, IntPtr.Zero, x - l, y - t, cw + l + r, ch + t + b, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}
