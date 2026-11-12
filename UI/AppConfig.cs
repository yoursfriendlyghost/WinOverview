using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Windows.Input;
using static WinOverview.NativeMethods;

namespace WinOverview;

/// <summary>
/// User settings, stored in %AppData%\WinOverview\config.json.
/// Changed from the Settings window (tray icon → Settings), or by editing the file.
/// </summary>
public sealed class AppConfig
{
    /// <summary>e.g. "Alt+`", "Ctrl+Alt+Space", "Win+Shift+Tab". Win+Tab itself is reserved by Windows.</summary>
    public string Hotkey { get; set; } = "Alt+`";

    /// <summary>"icons" (app icons in a grid) or "previews" (live mini-screens of each desktop).</summary>
    public string Mode { get; set; } = "icons";

    [JsonIgnore] public bool IsPreviewMode => string.Equals(Mode, "previews", StringComparison.OrdinalIgnoreCase);

    /// <summary>Highlight colour, as #RRGGBB.</summary>
    public string AccentColor { get; set; } = "#8AB4F8";

    /// <summary>"top" or "center".</summary>
    public string PanelPosition { get; set; } = "top";

    /// <summary>Gap above the panel when PanelPosition is "top", as % of screen height.</summary>
    public double TopMarginPercent { get; set; } = 10;

    public int DesktopsPerRow { get; set; } = 5;

    public bool HoverPreview { get; set; } = true;
    public int HoverPreviewDelayMs { get; set; } = 400;

    /// <summary>
    /// What's behind the panel: "clear" (your screen, untouched), "blur" (your screen, blurred)
    /// or "wallpaper" (your wallpaper, blurred).
    /// </summary>
    public string Background { get; set; } = "clear";

    /// <summary>How much to darken a "blur" or "wallpaper" background, 0 (none) to 1 (black).</summary>
    public double BackgroundDim { get; set; } = 0.4;

    public bool IsBackground(string style) => string.Equals(Background, style, StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Only matters if your taskbar is set to auto-hide. true = show it while the overview is open
    /// (Windows then briefly resizes maximized apps to make room); false = leave it hidden
    /// (move the mouse to the bottom edge to bring it up, as usual).
    /// </summary>
    public bool ShowAutoHiddenTaskbar { get; set; } = false;

    /// <summary>Widgets under the desktops, in order. Ids from <see cref="AvailableWidgets"/>.</summary>
    public List<string> Widgets { get; set; } = new() { "pomodoro", "music" };

    /// <summary>Every widget that can be added: id → name shown in Settings.</summary>
    public static readonly (string Id, string Name)[] AvailableWidgets =
    {
        ("pomodoro", "Focus timer (Pomodoro)"),
        ("music", "Music player"),
    };

    public bool HasWidget(string id) => Widgets.Any(w => string.Equals(w, id, StringComparison.OrdinalIgnoreCase));

    public int PomodoroFocusMinutes { get; set; } = 25;
    public int PomodoroBreakMinutes { get; set; } = 5;
    public int PomodoroLongBreakMinutes { get; set; } = 15;
    public int PomodoroRoundsBeforeLongBreak { get; set; } = 4;

    /// <summary>Open on the monitor the mouse is on (otherwise always the main monitor).</summary>
    public bool OpenOnMouseMonitor { get; set; } = true;

    // ---------------------------------------------------------------------------------

    public static string Folder =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "WinOverview");

    public static string FilePath => Path.Combine(Folder, "config.json");

    private static readonly JsonSerializerOptions Json = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
        ReadCommentHandling = JsonCommentHandling.Skip,
        AllowTrailingCommas = true,
    };

    public static AppConfig Load()
    {
        try
        {
            if (File.Exists(FilePath))
                return JsonSerializer.Deserialize<AppConfig>(File.ReadAllText(FilePath), Json) ?? new AppConfig();
        }
        catch
        {
            // Broken JSON (e.g. half-saved) → defaults; fixing the file reloads it
        }
        return new AppConfig();
    }

    public static void Save(AppConfig config)
    {
        Directory.CreateDirectory(Folder);
        File.WriteAllText(FilePath, JsonSerializer.Serialize(config, Json));
    }

    public AppConfig Clone() =>
        JsonSerializer.Deserialize<AppConfig>(JsonSerializer.Serialize(this, Json), Json) ?? new AppConfig();

    /// <summary>Writes the defaults if there's no config yet. Returns true on first run.</summary>
    public static bool EnsureExists()
    {
        if (File.Exists(FilePath)) return false;
        Directory.CreateDirectory(Folder);
        File.WriteAllText(FilePath, JsonSerializer.Serialize(new AppConfig(), Json));
        return true;
    }

    /// <summary>Calls <paramref name="onChange"/> (on a background thread) whenever config.json is saved.</summary>
    public static FileSystemWatcher Watch(Action onChange)
    {
        Directory.CreateDirectory(Folder);
        var watcher = new FileSystemWatcher(Folder, "config.json")
        {
            NotifyFilter = NotifyFilters.LastWrite | NotifyFilters.Size | NotifyFilters.FileName,
        };
        watcher.Changed += (_, _) => onChange();
        watcher.Created += (_, _) => onChange();
        watcher.Renamed += (_, _) => onChange();
        watcher.EnableRaisingEvents = true;
        return watcher;
    }

    /// <summary>Turns "Ctrl+Alt+Space" into RegisterHotKey's modifiers + virtual-key code.</summary>
    public bool TryParseHotkey(out uint modifiers, out uint vk)
    {
        modifiers = 0;
        vk = 0;
        foreach (string part in Hotkey.Split('+', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries))
        {
            switch (part.ToLowerInvariant())
            {
                case "alt": modifiers |= MOD_ALT; break;
                case "ctrl": case "control": modifiers |= MOD_CONTROL; break;
                case "shift": modifiers |= MOD_SHIFT; break;
                case "win": case "windows": modifiers |= MOD_WIN; break;
                default:
                    vk = KeyNameToVk(part);
                    if (vk == 0) return false;
                    break;
            }
        }
        return vk != 0;
    }

    private static uint KeyNameToVk(string name)
    {
        switch (name.ToLowerInvariant())
        {
            case "`": case "~": case "backtick": return VK_OEM_3;
            case "tab": return 0x09;
            case "space": return 0x20;
            case "esc": case "escape": return 0x1B;
        }
        if (name.Length == 1 && char.IsLetterOrDigit(name[0])) return char.ToUpperInvariant(name[0]);
        if (Enum.TryParse(name, true, out Key key)) return (uint)KeyInterop.VirtualKeyFromKey(key);
        return 0;
    }
}
