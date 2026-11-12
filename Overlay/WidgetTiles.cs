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

// The widget tiles under the desktops: focus timer and music player.
public partial class MainWindow
{
    // ================= Widgets: Pomodoro + music =================

    private const string IconFont = "Segoe Fluent Icons, Segoe MDL2 Assets";
    private const string GlyphPlay = "\uE768", GlyphPause = "\uE769", GlyphPrev = "\uE892",
                         GlyphNext = "\uE893", GlyphReset = "\uE72C", GlyphMusic = "\uE8D6";

    private void BuildWidgets()
    {
        WidgetRow.Children.Clear();
        _pomoTime = _musicTitle = null;
        _vinyl = null;
        _vinylSpinning = false;
        foreach (string id in _config.Widgets.Distinct(StringComparer.OrdinalIgnoreCase))
        {
            if (id.Equals("pomodoro", StringComparison.OrdinalIgnoreCase)) WidgetRow.Children.Add(MakePomodoroTile());
            else if (id.Equals("music", StringComparison.OrdinalIgnoreCase)) WidgetRow.Children.Add(MakeMusicTile());
        }
        UpdateWidgets();
    }

    /// <summary>Same box as a desktop tile: pill on top, content below.</summary>
    private static Border MakeWidgetTile(Border pill, FrameworkElement body)
    {
        pill.Margin = new Thickness(0, 0, 0, 10);
        var layout = new DockPanel
        {
            Width = TileW - 2 * TilePad,
            Height = TileH - 2 * TilePad,
            Background = Brushes.Transparent,
            LastChildFill = true,
        };
        DockPanel.SetDock(pill, Dock.Top);
        layout.Children.Add(pill);
        layout.Children.Add(body);

        var tile = new Border
        {
            Child = layout,
            Margin = new Thickness(5),
            Padding = new Thickness(TilePad - 1),
            CornerRadius = new CornerRadius(14),
            Background = TileBg,
            BorderThickness = new Thickness(1),
            BorderBrush = Brushes.Transparent,
        };
        tile.MouseEnter += (_, _) => tile.Background = TileHoverBg;
        tile.MouseLeave += (_, _) => tile.Background = TileBg;
        return tile;
    }

    private static Border MakeTextPill(string text, out TextBlock label)
    {
        label = new TextBlock
        {
            Text = text,
            FontSize = 11,
            FontWeight = FontWeights.SemiBold,
            Foreground = PillText,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        };
        return new Border
        {
            Height = PillHeight,
            Padding = new Thickness(9, 0, 9, 0),
            CornerRadius = new CornerRadius(PillHeight / 2),
            Background = PillBg,
            HorizontalAlignment = HorizontalAlignment.Left,
            Child = label,
        };
    }

    private static Border MakeIconButton(string glyph, string tooltip, Action onClick, out TextBlock glyphText, double size = 30)
    {
        glyphText = new TextBlock
        {
            Text = glyph,
            FontFamily = new FontFamily(IconFont),
            FontSize = 13,
            Foreground = Brushes.White,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        };
        var button = new Border
        {
            Width = size,
            Height = size,
            Margin = new Thickness(3, 0, 3, 0),
            CornerRadius = new CornerRadius(9),
            Background = CellBg,
            Cursor = Cursors.Hand,
            ToolTip = tooltip,
            Child = glyphText,
        };
        button.MouseEnter += (_, _) => button.Background = CellHoverBg;
        button.MouseLeave += (_, _) => button.Background = CellBg;
        button.MouseLeftButtonDown += (_, e) => e.Handled = true;
        button.MouseLeftButtonUp += (_, e) =>
        {
            e.Handled = true;
            onClick();
        };
        return button;
    }

    private Border MakePomodoroTile()
    {
        var pill = MakeTextPill("Focus", out var pillText);
        _pomoPill = pillText;
        _pomoPillBorder = pill;

        _pomoTime = new TextBlock
        {
            FontSize = 30,
            FontWeight = FontWeights.SemiBold,
            Foreground = Brushes.White,
            HorizontalAlignment = HorizontalAlignment.Center,
        };
        _pomoRounds = new TextBlock
        {
            FontSize = 10,
            Foreground = FaintText,
            HorizontalAlignment = HorizontalAlignment.Center,
            Margin = new Thickness(0, -2, 0, 6),
        };

        var buttons = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Center };
        buttons.Children.Add(MakeIconButton(GlyphReset, "Reset", () => { _pomodoro.Reset(); UpdateWidgets(); }, out _));
        buttons.Children.Add(MakeIconButton(GlyphPlay, "Start / pause", () => { _pomodoro.StartPause(); UpdateWidgets(); }, out var playGlyph));
        buttons.Children.Add(MakeIconButton(GlyphNext, "Skip to next", () => { _pomodoro.Skip(); UpdateWidgets(); }, out _));
        _pomoPlayGlyph = playGlyph;

        // Thin progress bar along the bottom
        _pomoBar = new Border { Height = 3, CornerRadius = new CornerRadius(1.5), Background = Accent, HorizontalAlignment = HorizontalAlignment.Left };
        var track = new Border
        {
            Height = 3,
            CornerRadius = new CornerRadius(1.5),
            Background = PillBg,
            Margin = new Thickness(0, 8, 0, 0),
            Child = _pomoBar,
        };

        var body = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        body.Children.Add(_pomoTime);
        body.Children.Add(_pomoRounds);
        body.Children.Add(buttons);
        body.Children.Add(track);
        return MakeWidgetTile(pill, body);
    }

    private Border MakeMusicTile()
    {
        var pill = MakeTextPill("Music", out _);

        _vinyl = MakeVinyl();
        _musicTitle = new TextBlock
        {
            FontSize = 12,
            FontWeight = FontWeights.SemiBold,
            Foreground = Brushes.White,
            TextTrimming = TextTrimming.CharacterEllipsis,
        };
        _musicArtist = new TextBlock
        {
            FontSize = 11,
            Foreground = FaintText,
            TextTrimming = TextTrimming.CharacterEllipsis,
            Margin = new Thickness(0, 2, 0, 0),
        };
        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(10, 0, 0, 0) };
        text.Children.Add(_musicTitle);
        text.Children.Add(_musicArtist);

        var top = new DockPanel { LastChildFill = true };
        DockPanel.SetDock(_vinyl, Dock.Left);
        top.Children.Add(_vinyl);
        top.Children.Add(text);

        var controls = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            HorizontalAlignment = HorizontalAlignment.Center,
            Margin = new Thickness(0, 12, 0, 0),
        };
        controls.Children.Add(MakeIconButton(GlyphPrev, "Previous", () => MediaAction(MediaController.PreviousAsync), out _));
        controls.Children.Add(MakeIconButton(GlyphPlay, "Play / pause", () => MediaAction(MediaController.PlayPauseAsync), out var playGlyph));
        controls.Children.Add(MakeIconButton(GlyphNext, "Next", () => MediaAction(MediaController.NextAsync), out _));
        _musicPlayGlyph = playGlyph;
        _musicControls = controls;

        var body = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        body.Children.Add(top);
        body.Children.Add(controls);

        // Click anywhere on the tile (except the buttons) → go to the player
        var tile = MakeWidgetTile(pill, body);
        tile.Cursor = Cursors.Hand;
        tile.ToolTip = "Open the player";
        tile.MouseLeftButtonUp += (_, e) =>
        {
            if (e.Handled) return;
            e.Handled = true;
            OpenMediaSource();
        };
        return tile;
    }

    /// <summary>A little vinyl record; spins while music plays, album art becomes its label.</summary>
    private Grid MakeVinyl()
    {
        const double size = 44;
        var disc = new Grid { Width = size, Height = size, RenderTransformOrigin = new Point(0.5, 0.5) };
        var rotate = new RotateTransform();
        disc.RenderTransform = rotate;

        disc.Children.Add(new Ellipse
        {
            Fill = new RadialGradientBrush(Color.FromRgb(0x2A, 0x2A, 0x30), Color.FromRgb(0x0C, 0x0C, 0x10)),
        });
        foreach (double d in new[] { 40.0, 35, 30, 25 }) // grooves
        {
            disc.Children.Add(new Ellipse
            {
                Width = d,
                Height = d,
                Stroke = Frozen(0x1C, 0xFF, 0xFF, 0xFF),
                StrokeThickness = 0.6,
            });
        }
        _vinylLabel = new Ellipse { Width = 18, Height = 18 };
        disc.Children.Add(_vinylLabel);
        disc.Children.Add(new Ellipse { Width = 3, Height = 3, Fill = Brushes.Black }); // spindle hole

        // Light streak across the disc, so you can see it turning
        disc.Children.Add(new Ellipse
        {
            Fill = new LinearGradientBrush(
                new GradientStopCollection
                {
                    new GradientStop(Colors.Transparent, 0.30),
                    new GradientStop(Color.FromArgb(0x30, 0xFF, 0xFF, 0xFF), 0.5),
                    new GradientStop(Colors.Transparent, 0.70),
                },
                new Point(0, 0), new Point(1, 1)),
            IsHitTestVisible = false,
        });

        var spin = new DoubleAnimation(0, 360, TimeSpan.FromSeconds(4)) { RepeatBehavior = RepeatBehavior.Forever };
        Storyboard.SetTarget(spin, disc);
        Storyboard.SetTargetProperty(spin, new PropertyPath("(UIElement.RenderTransform).(RotateTransform.Angle)"));
        _vinylSpin = new Storyboard { Children = { spin } };
        _vinylSpin.Begin(disc, true); // controllable, so it can pause/resume where it is
        _vinylSpin.Pause(disc);
        _vinylSpinning = false;

        SetVinylLabel(null);
        return disc;
    }

    private void SetVinylLabel(ImageSource? art)
    {
        if (_vinylLabel == null) return;
        _vinylLabel.Fill = art != null
            ? new ImageBrush(art) { Stretch = Stretch.UniformToFill }
            : new LinearGradientBrush(
                ((SolidColorBrush)Accent).Color,
                Color.FromRgb(0x3A, 0x3A, 0x55), 45);
    }

    private void SetVinylSpinning(bool spinning)
    {
        if (_vinyl == null || _vinylSpin == null || spinning == _vinylSpinning) return;
        if (spinning) _vinylSpin.Resume(_vinyl); else _vinylSpin.Pause(_vinyl);
        _vinylSpinning = spinning;
    }

    private static readonly HashSet<string> Browsers = new(StringComparer.OrdinalIgnoreCase)
    {
        "zen", "firefox", "librewolf", "floorp", "waterfox", "chrome", "msedge", "brave", "opera", "vivaldi", "arc",
    };

    /// <summary>Brings up whatever is playing: the player app, or the browser window with that tab.</summary>
    private void OpenMediaSource()
    {
        if (!_media.HasSession) return;

        // Program name of every open window, e.g. "spotify", "chrome", "zen"
        var windows = _windows
            .Select(w => (Window: w, Exe: System.IO.Path.GetFileNameWithoutExtension(IconHelper.GetExePath(w.Handle) ?? "")))
            .ToList();

        // The player's id from Windows, cleaned up: "Spotify.exe" → "Spotify", "Pub.App_id!Spotify" → "Spotify"
        string app = _media.SourceApp;
        int bang = app.LastIndexOf('!');
        if (bang >= 0) app = app[(bang + 1)..];
        if (app.EndsWith(".exe", StringComparison.OrdinalIgnoreCase)) app = app[..^4];

        // 1. A window whose title contains the song (the playing tab is the active one, most players)
        var match = _media.Title.Length > 2
            ? windows.FirstOrDefault(x => x.Window.Title.Contains(_media.Title, StringComparison.OrdinalIgnoreCase)).Window
            : null;

        // 2. A window of the player's program ("MSEdge" ↔ msedge.exe, "Chrome" ↔ chrome.exe…)
        if (match == null && app.Length > 0)
            match = windows.FirstOrDefault(x => x.Exe.Length > 0 &&
                (x.Exe.Equals(app, StringComparison.OrdinalIgnoreCase) ||
                 app.Contains(x.Exe, StringComparison.OrdinalIgnoreCase))).Window;

        // 3. Firefox-based browsers (Firefox, Zen, LibreWolf…) report a random-looking id instead of
        //    their name, so fall back to the most recently used browser window
        if (match == null)
            match = windows.FirstOrDefault(x => Browsers.Contains(x.Exe)).Window;

        if (match != null)
        {
            FocusWindow(match);
            return;
        }

        // 3. A Store app with no window open → launch it
        if (_media.SourceApp.Contains('!'))
        {
            string aumid = _media.SourceApp;
            HideOverview(() => System.Diagnostics.Process.Start(
                new System.Diagnostics.ProcessStartInfo("explorer.exe", @"shell:AppsFolder\" + aumid) { UseShellExecute = true }));
        }
    }

    private async void MediaAction(Func<Task> action)
    {
        await action();
        await Task.Delay(250); // give the player a moment to update
        UpdateWidgets();
    }

    /// <summary>Refreshes the widget contents (every second while the overview is open).</summary>
    private async void UpdateWidgets()
    {
        // ---- Pomodoro ----
        if (_pomoTime != null)
        {
            var left = _pomodoro.Remaining;
            _pomoTime.Text = $"{(int)left.TotalMinutes:00}:{left.Seconds:00}";
            _pomoPill!.Text = _pomodoro.PhaseName;
            bool active = _pomodoro.Running;
            _pomoPillBorder!.Background = active ? Accent : PillBg;
            _pomoPill.Foreground = active ? AccentText : PillText;
            _pomoPlayGlyph!.Text = active ? GlyphPause : GlyphPlay;
            int done = _pomodoro.CompletedRounds % _pomodoro.RoundsBeforeLongBreak;
            _pomoRounds!.Text = $"round {done + 1} of {_pomodoro.RoundsBeforeLongBreak}";
            _pomoBar!.Width = Math.Max(0, (TileW - 2 * TilePad) * _pomodoro.Progress);
        }

        // ---- Music ----
        if (_musicTitle == null || _musicBusy) return;
        _musicBusy = true;
        var info = await MediaController.GetAsync();
        _musicBusy = false;
        if (_musicTitle == null) return;

        _musicTitle.Text = info.HasSession ? (info.Title.Length > 0 ? info.Title : "Unknown title") : "Nothing playing";
        _musicArtist!.Text = info.HasSession ? info.Artist : "Play something to control it here";
        _musicPlayGlyph!.Text = info.Playing ? GlyphPause : GlyphPlay;
        _musicControls!.Opacity = info.HasSession ? 1 : 0.4;
        _musicControls.IsEnabled = info.HasSession;

        _media = info;
        SetVinylLabel(info.Art);
        SetVinylSpinning(info.Playing);
    }
}
