using System.IO;
using System.Windows;

namespace WinOverview;

public partial class App : Application
{
    private Mutex? _singleInstance;
    private bool _ownsMutex;
    private bool _cleanedUp;
    private MainWindow? _overlay;
    private TrayIcon? _tray;
    private FileSystemWatcher? _configWatcher;
    private CancellationTokenSource? _reloadDebounce;
    private SettingsWindow? _settings;
    private DateTime _lastSelfSave;
    private bool _errorShown;

    protected override void OnStartup(StartupEventArgs e)
    {
        // Only one copy at a time
        _singleInstance = new Mutex(true, "WinOverview.SingleInstance", out bool isFirst);
        if (!isFirst && !WaitForOldCopy(e.Args))
        {
            Environment.Exit(0);
            return;
        }
        _ownsMutex = true;

        base.OnStartup(e);

        // Anything unexpected: write it to error.log, tidy up, and keep running
        DispatcherUnhandledException += (_, args) =>
        {
            args.Handled = true;
            ErrorLog.Write(args.Exception);
            _overlay?.ResetAfterError();
            if (!_errorShown)
            {
                _errorShown = true;
                _tray?.ShowTip("WinOverview hit a problem", "It recovered and is still running. Details are in Settings → Advanced → Open error log.");
            }
        };
        AppDomain.CurrentDomain.UnhandledException += (_, args) =>
        {
            if (args.ExceptionObject is Exception ex) ErrorLog.Write(ex);
        };
        TaskScheduler.UnobservedTaskException += (_, args) =>
        {
            ErrorLog.Write(args.Exception);
            args.SetObserved();
        };

        bool firstRun = AppConfig.EnsureExists();
        _overlay = new MainWindow(AppConfig.Load());
        if (_overlay.FailedToStart)
        {
            Quit();
            return;
        }

        _tray = new TrayIcon(open: () => _overlay.OpenFromTray(), settings: OpenSettings, restart: Restart, quit: Quit);
        _tray.SetTooltip($"WinOverview — {_overlay.HotkeyText}");
        _overlay.PomodoroFinished += message =>
        {
            _tray?.ShowTip("Pomodoro", message);
            System.Media.SystemSounds.Asterisk.Play();
        };
        if (firstRun) _tray.ShowTip("WinOverview is running", $"Press {_overlay.HotkeyText} to open it. Right-click this icon for settings.");

        // Apply config.json changes as soon as the file is saved
        _configWatcher = AppConfig.Watch(ScheduleReload);
    }

    private void OpenSettings()
    {
        if (_settings != null)
        {
            _settings.Activate();
            return;
        }
        _settings = new SettingsWindow(AppConfig.Load(), ApplySettings, paused => _overlay?.PauseHotkey(paused),
                                       _overlay?.LastOpenMs);
        _settings.Closed += (_, _) =>
        {
            _settings = null;
            _overlay?.PauseHotkey(false);
        };
        _settings.Show();
        _settings.Activate();
    }

    /// <summary>A change made in the Settings window: save it and apply it.</summary>
    private void ApplySettings(AppConfig config)
    {
        _lastSelfSave = DateTime.UtcNow;   // the file watcher will see our own save; skip that one
        AppConfig.Save(config);
        if (_overlay == null) return;
        _overlay.ApplyConfig(config);
        _tray?.SetTooltip($"WinOverview — {_overlay.HotkeyText}");
    }

    /// <summary>Tidy up (tray icon, taskbar, single-instance lock) and end the process for sure.</summary>
    public void Quit()
    {
        try { CleanUp(); } catch { /* quitting anyway */ }

        // Environment.Exit can hang while Windows tidies up the media-controls (WinRT/COM) objects,
        // so end the process directly. Everything worth saving is already saved.
        System.Diagnostics.Process.GetCurrentProcess().Kill();
    }

    /// <summary>Starts a fresh copy (which waits for this one to exit), then quits.</summary>
    private void Restart()
    {
        if (Environment.ProcessPath is { } exe)
        {
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(exe, "--restart")
            {
                UseShellExecute = false,
                WorkingDirectory = AppContext.BaseDirectory,
            });
        }
        Quit();
    }

    private void CleanUp()
    {
        if (_cleanedUp) return;
        _cleanedUp = true;

        _configWatcher?.Dispose();
        _tray?.Dispose();                // removes the tray icon right away (no ghost icon)
        TaskbarHelper.Restore();
        if (_ownsMutex)
        {
            try { _singleInstance?.ReleaseMutex(); } catch { /* already released */ }
            _ownsMutex = false;
        }
    }

    /// <summary>When launched by Restart, wait for the old copy to quit instead of exiting.</summary>
    private bool WaitForOldCopy(string[] args)
    {
        if (!args.Contains("--restart") || _singleInstance == null) return false;
        try
        {
            return _singleInstance.WaitOne(TimeSpan.FromSeconds(10));
        }
        catch (AbandonedMutexException)
        {
            return true; // old copy exited without releasing it — it's ours now
        }
    }

    private async void ScheduleReload()
    {
        // Editors save in several steps; wait for them to finish
        _reloadDebounce?.Cancel();
        var cts = _reloadDebounce = new CancellationTokenSource();
        try { await Task.Delay(300, cts.Token); } catch (TaskCanceledException) { return; }

        if ((DateTime.UtcNow - _lastSelfSave).TotalSeconds < 1.5) return; // our own save

        await Dispatcher.InvokeAsync(() =>
        {
            if (_overlay == null) return;
            _overlay.ApplyConfig(AppConfig.Load());
            _tray?.SetTooltip($"WinOverview — {_overlay.HotkeyText}");
        });
    }

    protected override void OnExit(ExitEventArgs e)
    {
        CleanUp();
        base.OnExit(e);
    }
}

/// <summary>Problems are written to %AppData%\WinOverview\error.log (kept small).</summary>
internal static class ErrorLog
{
    public static string FilePath => Path.Combine(AppConfig.Folder, "error.log");

    public static void Write(Exception ex)
    {
        try
        {
            Directory.CreateDirectory(AppConfig.Folder);
            if (File.Exists(FilePath) && new FileInfo(FilePath).Length > 256 * 1024) File.Delete(FilePath);
            File.AppendAllText(FilePath, $"[{DateTime.Now:yyyy-MM-dd HH:mm:ss}] {ex}{Environment.NewLine}{Environment.NewLine}");
        }
        catch
        {
            // nowhere left to report it
        }
    }
}
