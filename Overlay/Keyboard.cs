using System.Windows;
using System.Windows.Input;
using System.Windows.Media;

namespace WinOverview;

// Keyboard control: arrows to pick an app, Enter to open it, type to search, and more.
//
//   ← → / Tab        previous / next app (across desktops)
//   ↑ ↓              row above / below, inside a desktop
//   Enter            open the selected app
//   Delete           close the selected app
//   Ctrl + 1–9       move the selected app to that desktop (one past the last makes a new one)
//   1–9              switch to that desktop (while not searching)
//   letters          search apps by name; Backspace edits, Esc clears
public partial class MainWindow
{
    private readonly List<IconItem> _navItems = new();   // every app on screen, in display order
    private IconItem? _selected;
    private string _query = "";

    private bool Searching => _query.Length > 0;

    private void OnKeyDown(object sender, KeyEventArgs e)
    {
        if (_closing) return;
        Key key = e.Key == Key.System ? e.SystemKey : e.Key;
        var mods = Keyboard.Modifiers;
        bool ctrl = mods.HasFlag(ModifierKeys.Control);

        switch (key)
        {
            case Key.Escape:
                e.Handled = true;
                if (_flyout != null) CloseFlyout();
                else if (Searching) SetQuery("");
                else HideOverview();
                return;

            case Key.Q when ctrl:
                e.Handled = true;
                QuitApp();
                return;

            case Key.Left or Key.Right or Key.Up or Key.Down:
                e.Handled = true;
                MoveSelection(key);
                return;

            case Key.Tab:
                e.Handled = true;
                MoveSelection(mods.HasFlag(ModifierKeys.Shift) ? Key.Left : Key.Right);
                return;

            case Key.Enter:
                e.Handled = true;
                if (_selected != null) FocusWindow(_selected.Info);
                return;

            case Key.Delete:
                e.Handled = true;
                if (_selected != null) CloseSelected();
                return;

            case Key.Back:
                if (Searching)
                {
                    e.Handled = true;
                    SetQuery(_query[..^1]);
                }
                return;
        }

        int digit = key is >= Key.D1 and <= Key.D9 ? key - Key.D1
                  : key is >= Key.NumPad1 and <= Key.NumPad9 ? key - Key.NumPad1
                  : -1;
        if (digit < 0) return;

        if (ctrl && _selected != null)
        {
            e.Handled = true;
            MoveSelectedTo(digit);
        }
        else if (!ctrl && !Searching)
        {
            e.Handled = true;   // also stops the digit from being typed into the search
            SwitchTo(digit);
        }
    }

    /// <summary>Letters typed while the overview is open go into the search.</summary>
    private void OnTextInput(object sender, TextCompositionEventArgs e)
    {
        if (_closing || Keyboard.Modifiers.HasFlag(ModifierKeys.Control) || Keyboard.Modifiers.HasFlag(ModifierKeys.Alt)) return;

        string text = new(e.Text.Where(c => !char.IsControl(c)).ToArray());
        if (text.Length == 0 || (!Searching && string.IsNullOrWhiteSpace(text))) return;

        e.Handled = true;
        SetQuery(_query + text);
    }

    // ================= Search =================

    private void SetQuery(string query)
    {
        _query = query;
        SearchBar.Visibility = Searching ? Visibility.Visible : Visibility.Collapsed;
        SearchText.Text = query;
        ApplySearch();

        if (Searching) _thumbHost?.Suspend();   // so the dimmed / highlighted apps are visible
        else if (_previewLayout) ShowThumbnails();
    }

    private void ApplySearch()
    {
        int matches = 0;
        foreach (var item in _navItems)
        {
            item.Matches = !Searching || Matches(item, _query);
            item.Cell.Opacity = item.Matches ? 1 : 0.22;
            if (item.Matches) matches++;
        }

        if (!Searching) return;
        SearchCount.Text = matches == 1 ? "1 app" : $"{matches} apps";

        // Keep the selection on a match; otherwise jump to the first one
        if (_selected == null || !_selected.Matches)
            Select(_navItems.FirstOrDefault(i => i.Matches));
    }

    private static bool Matches(IconItem item, string query)
    {
        if (item.Info.Title.Contains(query, StringComparison.OrdinalIgnoreCase)) return true;
        item.ExeName ??= System.IO.Path.GetFileNameWithoutExtension(IconHelper.GetExePath(item.Info.Handle) ?? "");
        return item.ExeName.Contains(query, StringComparison.OrdinalIgnoreCase);
    }

    // ================= Selection =================

    private void MoveSelection(Key direction)
    {
        var candidates = _navItems.Where(i => i.Matches).ToList();
        if (candidates.Count == 0) return;

        // First key press: start on the app you were using
        if (_selected == null || !candidates.Contains(_selected))
        {
            Select(candidates.FirstOrDefault(i => i.Info.Handle == _previousForeground) ?? candidates[0]);
            return;
        }

        int at = candidates.IndexOf(_selected);
        switch (direction)
        {
            case Key.Left:
                Select(candidates[(at - 1 + candidates.Count) % candidates.Count]);
                break;
            case Key.Right:
                Select(candidates[(at + 1) % candidates.Count]);
                break;
            case Key.Up or Key.Down:
            {
                // Same desktop, one row up/down in its grid
                int target = _selected.IndexInDesktop + (direction == Key.Up ? -_selected.DesktopCols : _selected.DesktopCols);
                var next = _navItems.FirstOrDefault(i =>
                    i.Info.Desktop == _selected.Info.Desktop && i.IndexInDesktop == target && i.Matches);
                if (next != null) Select(next);
                break;
            }
        }
    }

    private void Select(IconItem? item)
    {
        if (item == _selected) return;
        var old = _selected;
        _selected = item;
        if (old != null) PaintItem(old);
        if (item != null) PaintItem(item);
    }

    /// <summary>Outline + highlight for the selected (or hovered) app; plain otherwise.</summary>
    private void PaintItem(IconItem item)
    {
        bool lit = item == _selected || item == _hovered;
        item.Cell.BorderBrush = lit ? Accent : Brushes.Transparent;
        if (!item.IsPreview) item.Cell.Background = lit ? CellHoverBg : CellBg;
    }

    /// <summary>After the tiles are rebuilt: keep the same app selected and the search applied.</summary>
    private void RestoreKeyboardState(IntPtr selectedHandle)
    {
        _selected = null;
        var again = _navItems.FirstOrDefault(i => i.Info.Handle == selectedHandle);
        ApplySearch();
        if (again != null && again.Matches) Select(again);
    }

    private void ResetKeyboardState()
    {
        _navItems.Clear();
        _selected = null;
        _query = "";
        SearchBar.Visibility = Visibility.Collapsed;
        SearchText.Text = "";
    }

    // ================= Keyboard actions =================

    private void MoveSelectedTo(int desktop)
    {
        if (_selected == null) return;
        var w = _selected.Info;
        int count = VirtualDesktop.GetDesktopCount();
        if (desktop == w.Desktop || desktop > count) return;   // one past the last = a new desktop

        int destination = desktop == count ? CreateDesktop() : desktop;
        if (destination < 0 || VirtualDesktop.MoveWindowToDesktopNumber(w.Handle, destination) < 0) return;

        _justMoved = w.Handle;   // pops in at its new place; stays selected
        Refresh();
    }

    private void CloseSelected()
    {
        if (_selected == null) return;

        // Select the next app, so you can keep pressing Delete
        var candidates = _navItems.Where(i => i.Matches).ToList();
        int at = candidates.IndexOf(_selected);
        var next = candidates.Count > 1 ? candidates[at + 1 < candidates.Count ? at + 1 : at - 1] : null;

        var closing = _selected.Info;
        Select(next);
        CloseWindow(closing);
    }
}
