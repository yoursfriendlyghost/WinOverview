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

// Live previews: the thumbnail grid ("previews" view) and the hover preview (icons view).
public partial class MainWindow
{
    // ================= Live previews ("previews" mode) =================

    /// <summary>Puts a live preview over every mini-window, once the panel has settled.</summary>
    private void ShowThumbnails()
    {
        if (!_previewLayout || !IsVisible || _closing || !_openAnimDone || _flyout != null || _dragging || Searching) return;

        var items = new List<(IntPtr, RECT, RECT)>();
        foreach (var it in _previewItems)
        {
            if (PresentationSource.FromVisual(it.Cell) == null || it.Cell.ActualWidth <= 0) continue;
            Point a = it.Cell.PointToScreen(new Point(Outline, Outline));
            Point b = it.Cell.PointToScreen(new Point(it.Cell.ActualWidth - Outline, it.Cell.ActualHeight - Outline));
            items.Add((it.Info.Handle, ToRect(a, b), it.Source));
        }
        if (items.Count == 0)
        {
            _thumbHost?.Clear();
            return;
        }

        Point c0 = Card.PointToScreen(new Point(0, 0));
        Point c1 = Card.PointToScreen(new Point(Card.ActualWidth, Card.ActualHeight));
        double dpi = VisualTreeHelper.GetDpi(this).DpiScaleX;

        _thumbHost ??= new ThumbnailHost(_hwnd);
        _thumbHost.Show(ToRect(c0, c1), items, (int)Math.Ceiling(15 * dpi));
    }

    private static RECT ToRect(Point a, Point b) =>
        new((int)Math.Round(a.X), (int)Math.Round(a.Y), (int)Math.Round(b.X), (int)Math.Round(b.Y));

    /// <summary>Bring the previews back after a drag or the snap picker.</summary>
    private void ResumeThumbnails()
    {
        if (_thumbHost != null && IsVisible && !_closing && _flyout == null && !_dragging && !Searching) _thumbHost.Resume();
    }


    // ================= Hover preview =================

    private void ShowPreview(IconItem item)
    {
        if (_preview == null || !item.Cell.IsLoaded) return;

        // Cell position on screen (physical pixels)
        Point a = item.Cell.PointToScreen(new Point(0, 0));
        Point b = item.Cell.PointToScreen(new Point(item.Cell.ActualWidth, item.Cell.ActualHeight));
        double scale = VisualTreeHelper.GetDpi(this).DpiScaleX;
        _preview.ShowFor(item.Info.Handle, item.Info.Title, new Rect(a, b), _monitorPx, scale);
    }

    private void HidePreview()
    {
        _hoverTimer.Stop();
        _preview?.HidePreview();
    }
}
