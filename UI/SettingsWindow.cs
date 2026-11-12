using System.Diagnostics;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Shapes;

namespace WinOverview;

/// <summary>The Settings window (tray icon → Settings…). Every change applies immediately.</summary>
internal sealed class SettingsWindow : Window
{
    private static readonly string[] Swatches =
    {
        "#8AB4F8", "#A78BFA", "#F472B6", "#F87171", "#FB923C", "#FACC15", "#4ADE80", "#2DD4BF",
    };

    private readonly AppConfig _config;
    private readonly Action<AppConfig> _apply;
    private readonly Action<bool> _pauseHotkey;
    private readonly StackPanel _page = new() { Margin = new Thickness(24, 12, 24, 24) };

    // Controls whose enabled state depends on other settings
    private FrameworkElement? _dimRow, _hoverRow, _hoverDelayRow, _topGapRow, _pomodoroBox;
    private StackPanel? _widgetList;
    private ComboBox? _widgetPicker;
    private readonly List<Border> _swatchRings = new();

    private readonly double? _lastOpenMs;

    public SettingsWindow(AppConfig config, Action<AppConfig> apply, Action<bool> pauseHotkey, double? lastOpenMs = null)
    {
        _lastOpenMs = lastOpenMs;
        _config = config.Clone();
        _apply = apply;
        _pauseHotkey = pauseHotkey;

#pragma warning disable WPF0001 // Windows 11 (Fluent) look, dark
        ThemeMode = ThemeMode.Dark;
#pragma warning restore WPF0001

        Title = "WinOverview Settings";
        Width = 560;
        Height = 760;
        MinWidth = 460;
        MinHeight = 400;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        FontFamily = new FontFamily("Segoe UI Variable Text, Segoe UI");

        Build();
        Content = new ScrollViewer
        {
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            Content = _page,
        };
    }

    /// <summary>Save + apply a copy (so later edits here don't change what the app holds).</summary>
    private void Changed()
    {
        UpdateEnabledStates();
        _apply(_config.Clone());
    }

    // ============================== Layout ==============================

    private void Build()
    {
        _page.Children.Add(new TextBlock
        {
            Text = "Settings",
            FontSize = 26,
            FontWeight = FontWeights.SemiBold,
            Margin = new Thickness(0, 0, 0, 4),
        });
        _page.Children.Add(Hint("Changes apply right away."));

        // ---------------- General ----------------
        Section("General");

        AddRow("Shortcut", "Click the box, then press the keys you want", HotkeyBox());

        AddRow("Start with Windows", null, Toggle(StartupHelper.Enabled, on => StartupHelper.Enabled = on, save: false));

        AddRow("Open on the screen with the mouse", "Otherwise always on the main screen",
               Toggle(_config.OpenOnMouseMonitor, on => _config.OpenOnMouseMonitor = on));

        AddRow("Show an auto-hidden taskbar", "Only if your taskbar auto-hides. Maximized apps briefly resize.",
               Toggle(_config.ShowAutoHiddenTaskbar, on => _config.ShowAutoHiddenTaskbar = on));

        // ---------------- Appearance ----------------
        Section("Appearance");

        AddRow("Desktop view", "App icons, or live mini-screens of each desktop",
               Choice(new[] { ("icons", "App icons"), ("previews", "Live previews") },
                      _config.IsPreviewMode ? "previews" : "icons", v => _config.Mode = v));

        AddRow("Accent colour", null, AccentPicker());

        AddRow("Panel position", null,
               Choice(new[] { ("top", "Top"), ("center", "Centre") },
                      _config.PanelPosition.Equals("center", StringComparison.OrdinalIgnoreCase) ? "center" : "top",
                      v => _config.PanelPosition = v));

        _topGapRow = AddRow("Gap from the top", null,
            SliderWithValue(0, 40, _config.TopMarginPercent, 1, v => $"{v:0}%", v => _config.TopMarginPercent = v));

        AddRow("Desktops per row", null,
            SliderWithValue(2, 10, _config.DesktopsPerRow, 1, v => $"{v:0}", v => _config.DesktopsPerRow = (int)v));

        AddRow("Background", "What's behind the panel",
               Choice(new[] { ("clear", "Clear"), ("blur", "Blurred screen"), ("wallpaper", "Blurred wallpaper") },
                      _config.Background.ToLowerInvariant() is "blur" or "wallpaper" ? _config.Background.ToLowerInvariant() : "clear",
                      v => _config.Background = v));

        _dimRow = AddRow("Darken background", null,
            SliderWithValue(0, 0.9, _config.BackgroundDim, 0.05, v => $"{v * 100:0}%", v => _config.BackgroundDim = v));

        // ---------------- Behaviour ----------------
        Section("Behaviour");

        _hoverRow = AddRow("Preview on hover", "Rest the mouse on an app icon to see its window",
                           Toggle(_config.HoverPreview, on => _config.HoverPreview = on));

        _hoverDelayRow = AddRow("Hover delay", null,
            SliderWithValue(0, 1500, _config.HoverPreviewDelayMs, 50, v => $"{v:0} ms", v => _config.HoverPreviewDelayMs = (int)v));

        // ---------------- Widgets ----------------
        Section("Widgets");
        _page.Children.Add(Hint("Extras shown under your desktops. Remove them all for a plain task view."));

        _widgetPicker = new ComboBox { MinWidth = 200, Margin = new Thickness(0, 0, 8, 0) };
        var addButton = new Button { Content = "Add", MinWidth = 70 };
        addButton.Click += (_, _) => AddSelectedWidget();
        var addRow = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 10, 0, 6) };
        addRow.Children.Add(_widgetPicker);
        addRow.Children.Add(addButton);
        _page.Children.Add(addRow);

        _widgetList = new StackPanel();
        _page.Children.Add(_widgetList);

        var pomodoro = new StackPanel { Margin = new Thickness(0, 6, 0, 0) };
        pomodoro.Children.Add(new TextBlock { Text = "Focus timer", FontWeight = FontWeights.SemiBold, Margin = new Thickness(0, 8, 0, 0) });
        pomodoro.Children.Add(Row("Focus", null,
            SliderWithValue(5, 90, _config.PomodoroFocusMinutes, 5, v => $"{v:0} min", v => _config.PomodoroFocusMinutes = (int)v)));
        pomodoro.Children.Add(Row("Short break", null,
            SliderWithValue(1, 30, _config.PomodoroBreakMinutes, 1, v => $"{v:0} min", v => _config.PomodoroBreakMinutes = (int)v)));
        pomodoro.Children.Add(Row("Long break", null,
            SliderWithValue(5, 60, _config.PomodoroLongBreakMinutes, 5, v => $"{v:0} min", v => _config.PomodoroLongBreakMinutes = (int)v)));
        pomodoro.Children.Add(Row("Rounds before a long break", null,
            SliderWithValue(1, 8, _config.PomodoroRoundsBeforeLongBreak, 1, v => $"{v:0}", v => _config.PomodoroRoundsBeforeLongBreak = (int)v)));
        _pomodoroBox = pomodoro;
        _page.Children.Add(pomodoro);

        RefreshWidgets();

        // ---------------- Footer ----------------
        Section("Advanced");
        var footer = new WrapPanel { Margin = new Thickness(0, 8, 0, 0) };
        var openFile = new Button { Content = "Open config file", Margin = new Thickness(0, 0, 8, 8) };
        openFile.Click += (_, _) =>
        {
            AppConfig.EnsureExists();
            Process.Start(new ProcessStartInfo("notepad.exe", $"\"{AppConfig.FilePath}\"") { UseShellExecute = true });
        };
        var reset = new Button { Content = "Reset to defaults", Margin = new Thickness(0, 0, 8, 8) };
        reset.Click += (_, _) =>
        {
            if (MessageBox.Show(this, "Reset every setting to its default?", "WinOverview",
                                MessageBoxButton.YesNo, MessageBoxImage.Question) != MessageBoxResult.Yes) return;
            _apply(new AppConfig());
            Close(); // reopen to see the defaults
        };
        var openLog = new Button { Content = "Open error log", Margin = new Thickness(0, 0, 8, 8) };
        openLog.Click += (_, _) =>
        {
            if (System.IO.File.Exists(ErrorLog.FilePath))
                Process.Start(new ProcessStartInfo("notepad.exe", $"\"{ErrorLog.FilePath}\"") { UseShellExecute = true });
            else
                MessageBox.Show(this, "No errors so far.", "WinOverview");
        };
        footer.Children.Add(openFile);
        footer.Children.Add(openLog);
        footer.Children.Add(reset);
        _page.Children.Add(footer);
        _page.Children.Add(Hint(_lastOpenMs is double ms
            ? $"Last time, the overview opened in {ms:0} ms."
            : "Open the overview once to see how fast it opens."));

        UpdateEnabledStates();
    }

    private void UpdateEnabledStates()
    {
        bool clear = !_config.IsBackground("blur") && !_config.IsBackground("wallpaper");
        SetEnabled(_dimRow, !clear);
        SetEnabled(_topGapRow, !_config.PanelPosition.Equals("center", StringComparison.OrdinalIgnoreCase));
        SetEnabled(_hoverRow, !_config.IsPreviewMode);           // previews mode already shows the windows
        SetEnabled(_hoverDelayRow, !_config.IsPreviewMode && _config.HoverPreview);
        if (_pomodoroBox != null)
            _pomodoroBox.Visibility = _config.HasWidget("pomodoro") ? Visibility.Visible : Visibility.Collapsed;
    }

    private static void SetEnabled(FrameworkElement? row, bool enabled)
    {
        if (row == null) return;
        row.IsEnabled = enabled;
        row.Opacity = enabled ? 1 : 0.45;
    }

    // ============================== Widgets ==============================

    private void RefreshWidgets()
    {
        if (_widgetList == null || _widgetPicker == null) return;

        _widgetList.Children.Clear();
        foreach (string id in _config.Widgets.ToList())
        {
            string name = AppConfig.AvailableWidgets.FirstOrDefault(w => w.Id.Equals(id, StringComparison.OrdinalIgnoreCase)).Name ?? id;
            var remove = new Button { Content = "Remove", Padding = new Thickness(12, 4, 12, 4) };
            remove.Click += (_, _) =>
            {
                _config.Widgets.RemoveAll(w => w.Equals(id, StringComparison.OrdinalIgnoreCase));
                RefreshWidgets();
                Changed();
            };

            var row = new DockPanel { Margin = new Thickness(0, 4, 0, 0) };
            DockPanel.SetDock(remove, Dock.Right);
            row.Children.Add(remove);
            row.Children.Add(new TextBlock { Text = name, VerticalAlignment = VerticalAlignment.Center, FontSize = 14 });
            _widgetList.Children.Add(Card(row));
        }
        if (_config.Widgets.Count == 0)
            _widgetList.Children.Add(Hint("No widgets — just the desktops."));

        // Only offer widgets that aren't added yet
        _widgetPicker.Items.Clear();
        foreach (var (id, name) in AppConfig.AvailableWidgets.Where(w => !_config.HasWidget(w.Id)))
            _widgetPicker.Items.Add(new ComboBoxItem { Content = name, Tag = id });
        _widgetPicker.IsEnabled = _widgetPicker.Items.Count > 0;
        if (_widgetPicker.Items.Count > 0) _widgetPicker.SelectedIndex = 0;
        else _widgetPicker.Items.Add(new ComboBoxItem { Content = "All widgets added", IsEnabled = false });
    }

    private void AddSelectedWidget()
    {
        if (_widgetPicker?.SelectedItem is not ComboBoxItem { Tag: string id }) return;
        _config.Widgets.Add(id);
        RefreshWidgets();
        Changed();
    }

    // ============================== Building blocks ==============================

    private void Section(string title)
    {
        _page.Children.Add(new TextBlock
        {
            Text = title,
            FontSize = 18,
            FontWeight = FontWeights.SemiBold,
            Margin = new Thickness(0, 24, 0, 4),
        });
    }

    private static TextBlock Hint(string text) => new()
    {
        Text = text,
        Opacity = 0.65,
        TextWrapping = TextWrapping.Wrap,
        Margin = new Thickness(0, 2, 0, 0),
    };

    private static Border Card(UIElement child) => new()
    {
        Child = child,
        Padding = new Thickness(14, 10, 14, 10),
        Margin = new Thickness(0, 4, 0, 0),
        CornerRadius = new CornerRadius(8),
        Background = new SolidColorBrush(Color.FromArgb(0x14, 0xFF, 0xFF, 0xFF)),
    };

    /// <summary>Label (+ hint) on the left, control on the right, in a rounded card.</summary>
    private static Border Row(string label, string? hint, FrameworkElement control)
    {
        var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(0, 0, 16, 0) };
        text.Children.Add(new TextBlock { Text = label, FontSize = 14, TextWrapping = TextWrapping.Wrap });
        if (hint != null) text.Children.Add(new TextBlock { Text = hint, FontSize = 12, Opacity = 0.65, TextWrapping = TextWrapping.Wrap });

        control.VerticalAlignment = VerticalAlignment.Center;
        var grid = new Grid();
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        Grid.SetColumn(control, 1);
        grid.Children.Add(text);
        grid.Children.Add(control);
        return Card(grid);
    }

    private FrameworkElement AddRow(string label, string? hint, FrameworkElement control)
    {
        var row = Row(label, hint, control);
        _page.Children.Add(row);
        return row;
    }

    private CheckBox Toggle(bool value, Action<bool> set, bool save = true)
    {
        var box = new CheckBox { IsChecked = value, MinWidth = 0 };
        box.Click += (_, _) =>
        {
            set(box.IsChecked == true);
            if (save) Changed();
        };
        return box;
    }

    private ComboBox Choice((string Value, string Label)[] options, string current, Action<string> set)
    {
        var combo = new ComboBox { MinWidth = 170 };
        foreach (var (value, label) in options)
        {
            var item = new ComboBoxItem { Content = label, Tag = value };
            combo.Items.Add(item);
            if (value.Equals(current, StringComparison.OrdinalIgnoreCase)) combo.SelectedItem = item;
        }
        if (combo.SelectedItem == null) combo.SelectedIndex = 0;
        combo.SelectionChanged += (_, _) =>
        {
            if (combo.SelectedItem is ComboBoxItem { Tag: string v })
            {
                set(v);
                Changed();
            }
        };
        return combo;
    }

    private FrameworkElement SliderWithValue(double min, double max, double value, double step,
                                             Func<double, string> format, Action<double> set)
    {
        var slider = new Slider
        {
            Minimum = min,
            Maximum = max,
            Value = Math.Clamp(value, min, max),
            TickFrequency = step,
            IsSnapToTickEnabled = true,
            Width = 170,
        };
        var label = new TextBlock { Text = format(slider.Value), Width = 56, TextAlignment = TextAlignment.Right, Margin = new Thickness(8, 0, 0, 0), VerticalAlignment = VerticalAlignment.Center };
        slider.ValueChanged += (_, _) =>
        {
            label.Text = format(slider.Value);
            set(slider.Value);
        };
        // Save once you let go, not on every pixel of the drag
        slider.PreviewMouseLeftButtonUp += (_, _) => Changed();
        slider.KeyUp += (_, _) => Changed();

        var panel = new StackPanel { Orientation = Orientation.Horizontal };
        panel.Children.Add(slider);
        panel.Children.Add(label);
        return panel;
    }

    private FrameworkElement AccentPicker()
    {
        var panel = new WrapPanel { MaxWidth = 250 };
        foreach (string hex in Swatches)
        {
            var color = (Color)ColorConverter.ConvertFromString(hex);
            var dot = new Ellipse { Width = 22, Height = 22, Fill = new SolidColorBrush(color) };
            var ring = new Border
            {
                Width = 30,
                Height = 30,
                Margin = new Thickness(1),
                CornerRadius = new CornerRadius(15),
                BorderThickness = new Thickness(2),
                BorderBrush = Brushes.Transparent,
                Background = Brushes.Transparent,
                Child = dot,
                Cursor = Cursors.Hand,
                ToolTip = hex,
                Tag = hex,
            };
            ring.MouseLeftButtonUp += (_, _) =>
            {
                _config.AccentColor = hex;
                HighlightSwatch();
                Changed();
            };
            _swatchRings.Add(ring);
            panel.Children.Add(ring);
        }
        HighlightSwatch();
        return panel;
    }

    private void HighlightSwatch()
    {
        foreach (var ring in _swatchRings)
            ring.BorderBrush = string.Equals((string)ring.Tag, _config.AccentColor, StringComparison.OrdinalIgnoreCase)
                ? Brushes.White
                : Brushes.Transparent;
    }

    /// <summary>Click it, then press a key combination (e.g. Alt+` or Ctrl+Alt+Space).</summary>
    private TextBox HotkeyBox()
    {
        var box = new TextBox
        {
            Text = _config.Hotkey,
            IsReadOnly = true,
            IsReadOnlyCaretVisible = false,
            MinWidth = 170,
            TextAlignment = TextAlignment.Center,
            Cursor = Cursors.Hand,
        };

        // Turn the real hotkey off while typing, so pressing it doesn't open the overview
        box.GotKeyboardFocus += (_, _) => { _pauseHotkey(true); box.Text = "Press keys…"; };
        box.LostKeyboardFocus += (_, _) => { _pauseHotkey(false); box.Text = _config.Hotkey; };

        box.PreviewKeyDown += (_, e) =>
        {
            e.Handled = true;
            Key key = e.Key == Key.System ? e.SystemKey : e.Key;
            if (key is Key.LeftCtrl or Key.RightCtrl or Key.LeftAlt or Key.RightAlt or Key.LeftShift
                    or Key.RightShift or Key.LWin or Key.RWin) return;   // wait for the actual key
            if (key == Key.Escape) { Keyboard.ClearFocus(); FocusManager.SetFocusedElement(this, this); return; }

            var mods = Keyboard.Modifiers;
            bool isFunctionKey = key >= Key.F1 && key <= Key.F24;
            if (mods == ModifierKeys.None && !isFunctionKey) return;     // need Ctrl/Alt/Shift/Win

            var parts = new List<string>();
            if (mods.HasFlag(ModifierKeys.Control)) parts.Add("Ctrl");
            if (mods.HasFlag(ModifierKeys.Alt)) parts.Add("Alt");
            if (mods.HasFlag(ModifierKeys.Shift)) parts.Add("Shift");
            if (mods.HasFlag(ModifierKeys.Windows)) parts.Add("Win");
            parts.Add(KeyName(key));
            string hotkey = string.Join("+", parts);

            var test = new AppConfig { Hotkey = hotkey };
            if (!test.TryParseHotkey(out uint _, out uint _)) return;

            _config.Hotkey = hotkey;
            box.Text = hotkey;
            Changed();
            Keyboard.ClearFocus();  // done → hotkey goes live again
            FocusManager.SetFocusedElement(this, this);
        };
        return box;
    }

    private static string KeyName(Key key) => key switch
    {
        Key.Oem3 => "`",
        Key.Space => "Space",
        Key.Tab => "Tab",
        >= Key.D0 and <= Key.D9 => ((int)(key - Key.D0)).ToString(),
        >= Key.A and <= Key.Z => key.ToString(),
        _ => key.ToString(),
    };
}
