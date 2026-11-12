using System.Diagnostics;
using Microsoft.Win32;
using Forms = System.Windows.Forms;

namespace WinOverview;

/// <summary>"Start with Windows" (a value under the registry's Run key).</summary>
internal static class StartupHelper
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string RunName = "WinOverview";

    public static bool Enabled
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey);
            return key?.GetValue(RunName) is string;
        }
        set
        {
            using var key = Registry.CurrentUser.CreateSubKey(RunKey);
            if (value) key.SetValue(RunName, $"\"{Environment.ProcessPath}\"");
            else key.DeleteValue(RunName, false);
        }
    }
}

/// <summary>Icon in the system tray with the app's menu.</summary>
internal sealed class TrayIcon : IDisposable
{
    private readonly Forms.NotifyIcon _icon;

    public TrayIcon(Action open, Action settings, Action restart, Action quit)
    {
        var menu = new Forms.ContextMenuStrip();
        menu.Items.Add("Open overview", null, (_, _) => open());
        menu.Items.Add(new Forms.ToolStripSeparator());

        var startup = new Forms.ToolStripMenuItem("Start with Windows") { CheckOnClick = true };
        startup.Click += (_, _) => StartupHelper.Enabled = startup.Checked;
        menu.Items.Add(startup);
        menu.Opening += (_, _) => startup.Checked = StartupHelper.Enabled; // stay in sync with Settings

        menu.Items.Add("Settings…", null, (_, _) => settings());
        menu.Items.Add(new Forms.ToolStripSeparator());
        menu.Items.Add("Restart", null, (_, _) => restart());
        menu.Items.Add("Quit", null, (_, _) => quit());

        _icon = new Forms.NotifyIcon
        {
            Icon = LoadIcon(),
            Text = "WinOverview",
            ContextMenuStrip = menu,
            Visible = true,
        };
        _icon.MouseClick += (_, e) =>
        {
            if (e.Button == Forms.MouseButtons.Left) open();
        };
    }

    public void SetTooltip(string text) => _icon.Text = text.Length > 63 ? text[..63] : text;

    public void ShowTip(string title, string text) =>
        _icon.ShowBalloonTip(4000, title, text, Forms.ToolTipIcon.None);

    private static System.Drawing.Icon LoadIcon()
    {
        try
        {
            // The app icon embedded in WinOverview.exe (app.ico)
            if (Environment.ProcessPath is { } exe && System.Drawing.Icon.ExtractAssociatedIcon(exe) is { } icon)
                return icon;
        }
        catch
        {
            // fall through
        }
        return System.Drawing.SystemIcons.Application;
    }

    public void Dispose()
    {
        _icon.Visible = false;
        _icon.Dispose();
    }
}
