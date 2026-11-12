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

// Dragging apps between desktops.
public partial class MainWindow
{
    // ================= Drag and drop =================

    private void OnIconMouseMove(IconItem item, Point p)
    {
        if (_pressed != item) return;

        if (!_dragging)
        {
            if ((p - _pressPoint).Length < DragThreshold) return;
            BeginDrag(item);
        }

        if (_ghost != null)
        {
            Canvas.SetLeft(_ghost, p.X - Cell / 2);
            Canvas.SetTop(_ghost, p.Y - Cell / 2);
        }
        HighlightDropTarget(TileAt(p), item);
    }

    private void BeginDrag(IconItem item)
    {
        _dragging = true;
        HidePreview();
        CloseFlyout();
        _thumbHost?.Suspend(); // previews step aside so the dragged app is visible
        item.Cell.Opacity = 0.3;
        item.Cell.Background = CellBg;
        item.Cell.BorderBrush = Brushes.Transparent;

        var ghost = MakeCell(MakeIconVisual(item.Icon, item.Info.Title));
        ghost.Margin = new Thickness(0);
        ghost.Background = CellHoverBg;
        ghost.BorderBrush = Accent;
        ghost.RenderTransform = new ScaleTransform(1.1, 1.1, Cell / 2, Cell / 2);
        ghost.Effect = new DropShadowEffect { BlurRadius = 16, ShadowDepth = 4, Opacity = 0.5, Color = Colors.Black };
        _ghost = ghost;
        DragLayer.Children.Add(ghost);
    }

    private void OnIconMouseUp(IconItem item, Point p)
    {
        if (_pressed != item) return;

        bool wasDragging = _dragging;
        var target = TileAt(p);
        var ghost = _ghost;
        _ghost = null;
        _pressed = null;
        _dragging = false;
        HighlightDropTarget(null, null);

        // Plain click → focus the app
        if (!wasDragging || ghost == null)
        {
            if (ghost != null) DragLayer.Children.Remove(ghost);
            ResetCell(item);
            if (!wasDragging) FocusWindow(item.Info);
            return;
        }

        // Dropped outside the panel → snap layouts
        // (bounds check, because IsMouseOver stays true while the icon holds mouse capture)
        if (target == null)
        {
            var cardBounds = new Rect(Card.TranslatePoint(new Point(0, 0), Root), Card.RenderSize);
            if (cardBounds.Contains(p))
            {
                FlyHome(item, ghost); // dropped in a gap inside the panel
                return;
            }
            DragLayer.Children.Remove(ghost);
            ResetCell(item);
            ShowSnapFlyout(item, p);
            return;
        }

        // Same desktop → glide back
        if (target.Index == item.Info.Desktop)
        {
            FlyHome(item, ghost);
            return;
        }

        int destDesktop = target.IsNew ? CreateDesktop() : target.Index;
        bool moved = destDesktop >= 0
                     && VirtualDesktop.MoveWindowToDesktopNumber(item.Info.Handle, destDesktop) >= 0;
        if (!moved)
        {
            FlyHome(item, ghost); // e.g. an app running as admin
            return;
        }

        // Fly into the target desktop, shrinking, then rebuild — the app pops in there
        _justMoved = item.Info.Handle;
        item.Cell.BeginAnimation(OpacityProperty, new DoubleAnimation(0, TimeSpan.FromMilliseconds(150)));
        target.Root.BorderBrush = Accent;
        var center = target.Root.TranslatePoint(
            new Point(target.Root.ActualWidth / 2, target.Root.ActualHeight / 2 + 10), Root);
        FlyGhost(ghost, center, shrink: true, () => Refresh(0));
    }

    /// <summary>Glide the dragged icon back to where it came from.</summary>
    private void FlyHome(IconItem item, FrameworkElement ghost)
    {
        var home = item.Cell.TranslatePoint(new Point(item.Cell.ActualWidth / 2, item.Cell.ActualHeight / 2), Root);
        FlyGhost(ghost, home, shrink: false, () => ResetCell(item));
    }

    private void FlyGhost(FrameworkElement ghost, Point to, bool shrink, Action done)
    {
        var d = TimeSpan.FromMilliseconds(FlyMs);
        var move = new DoubleAnimation(Canvas.GetLeft(ghost), to.X - Cell / 2, d) { EasingFunction = EaseInOut };
        move.Completed += (_, _) =>
        {
            DragLayer.Children.Remove(ghost);
            done();
        };
        ghost.BeginAnimation(Canvas.LeftProperty, move);
        ghost.BeginAnimation(Canvas.TopProperty,
            new DoubleAnimation(Canvas.GetTop(ghost), to.Y - Cell / 2, d) { EasingFunction = EaseInOut });

        if (ghost.RenderTransform is ScaleTransform scale)
        {
            double endScale = shrink ? 0.5 : 1.0;
            scale.BeginAnimation(ScaleTransform.ScaleXProperty, new DoubleAnimation(endScale, d) { EasingFunction = EaseInOut });
            scale.BeginAnimation(ScaleTransform.ScaleYProperty, new DoubleAnimation(endScale, d) { EasingFunction = EaseInOut });
        }
        if (shrink)
            ghost.BeginAnimation(OpacityProperty,
                new DoubleAnimation(0.2, d) { BeginTime = TimeSpan.FromMilliseconds(FlyMs * 0.5), EasingFunction = EaseOut });
    }

    private void ResetCell(IconItem item)
    {
        item.Cell.BeginAnimation(OpacityProperty, null);
        item.Cell.Opacity = item.Matches ? 1 : 0.22;
        PaintItem(item);
        ResumeThumbnails();
    }

    private void CancelDrag()
    {
        var pressed = _pressed;
        _pressed = null;
        _dragging = false;

        if (_ghost != null)
        {
            DragLayer.Children.Remove(_ghost);
            _ghost = null;
        }
        HighlightDropTarget(null, null);
        if (pressed != null) ResetCell(pressed);
    }

    private DesktopTile? TileAt(Point p)
    {
        foreach (var tile in _tiles)
        {
            var topLeft = tile.Root.TranslatePoint(new Point(0, 0), Root);
            if (new Rect(topLeft, tile.Root.RenderSize).Contains(p)) return tile;
        }
        return null;
    }

    private void HighlightDropTarget(DesktopTile? target, IconItem? dragged)
    {
        foreach (var tile in _tiles)
        {
            bool isTarget = tile == target && dragged != null && tile.Index != dragged.Info.Desktop;
            tile.Root.BorderBrush = isTarget ? Accent : tile.IsNew ? NewTileBorder : Brushes.Transparent;
            tile.Root.Background = isTarget ? TileHoverBg : tile.IsNew ? Brushes.Transparent : TileBg;
        }
    }
}
