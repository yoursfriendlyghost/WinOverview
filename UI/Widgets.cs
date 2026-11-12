// Logic behind the widgets under the desktops: a Pomodoro timer and the "now playing" media controls.
using System.IO;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Windows.Media.Control;
using Windows.Storage.Streams;

namespace WinOverview;

// ===================== Pomodoro =====================
/// <summary>Focus / break timer. Keeps running while the overview is hidden.</summary>
internal sealed class Pomodoro
{
    public enum Phase { Focus, ShortBreak, LongBreak }

    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromMilliseconds(500) };
    private DateTime _endsAt;
    private TimeSpan _remaining;
    private int _focusMin = 25, _breakMin = 5, _longBreakMin = 15, _roundsBeforeLong = 4;

    public Phase Current { get; private set; } = Phase.Focus;
    public int CompletedRounds { get; private set; }
    public int RoundsBeforeLongBreak => _roundsBeforeLong;
    public bool Running => _timer.IsEnabled;

    /// <summary>Raised when a phase ends by itself, with a message for the user.</summary>
    public event Action<string>? Finished;

    public Pomodoro()
    {
        _timer.Tick += (_, _) =>
        {
            if (Remaining <= TimeSpan.Zero) Advance(finishedByItself: true);
        };
        _remaining = Duration(Current);
    }

    public TimeSpan Remaining
    {
        get
        {
            var left = Running ? _endsAt - DateTime.Now : _remaining;
            return left < TimeSpan.Zero ? TimeSpan.Zero : left;
        }
    }

    /// <summary>0 at the start of a phase, 1 at the end.</summary>
    public double Progress => 1 - Remaining.TotalSeconds / Math.Max(1, Duration(Current).TotalSeconds);

    public string PhaseName => Current switch
    {
        Phase.Focus => "Focus",
        Phase.ShortBreak => "Break",
        _ => "Long break",
    };

    public void Configure(int focusMin, int breakMin, int longBreakMin, int roundsBeforeLong)
    {
        _focusMin = Math.Clamp(focusMin, 1, 180);
        _breakMin = Math.Clamp(breakMin, 1, 60);
        _longBreakMin = Math.Clamp(longBreakMin, 1, 120);
        _roundsBeforeLong = Math.Clamp(roundsBeforeLong, 1, 12);
        if (!Running) _remaining = Duration(Current);
    }

    public void StartPause()
    {
        if (Running)
        {
            _remaining = Remaining;
            _timer.Stop();
        }
        else
        {
            _endsAt = DateTime.Now + _remaining;
            _timer.Start();
        }
    }

    public void Reset()
    {
        _timer.Stop();
        _remaining = Duration(Current);
    }

    public void Skip() => Advance(finishedByItself: false);

    private TimeSpan Duration(Phase p) => TimeSpan.FromMinutes(p switch
    {
        Phase.Focus => _focusMin,
        Phase.ShortBreak => _breakMin,
        _ => _longBreakMin,
    });

    private void Advance(bool finishedByItself)
    {
        bool wasRunning = Running;
        _timer.Stop();

        string message;
        if (Current == Phase.Focus)
        {
            CompletedRounds++;
            Current = CompletedRounds % _roundsBeforeLong == 0 ? Phase.LongBreak : Phase.ShortBreak;
            int minutes = Current == Phase.LongBreak ? _longBreakMin : _breakMin;
            message = $"Focus session done — take a {minutes}-minute break.";
        }
        else
        {
            Current = Phase.Focus;
            message = $"Break's over — time to focus for {_focusMin} minutes.";
        }
        _remaining = Duration(Current);

        // The next phase starts automatically when one finishes (or if you skip while running)
        if (finishedByItself || wasRunning)
        {
            _endsAt = DateTime.Now + _remaining;
            _timer.Start();
        }
        if (finishedByItself) Finished?.Invoke(message);
    }
}

// ===================== Media =====================
/// <summary>What's playing right now (Spotify, YouTube in a browser, Media Player…).</summary>
/// <param name="SourceApp">The player's app id, e.g. "Spotify.exe", "Chrome", "MSEdge".</param>
internal sealed record MediaInfo(bool HasSession, string Title, string Artist, bool Playing, ImageSource? Art, string SourceApp)
{
    public static readonly MediaInfo None = new(false, "", "", false, null, "");
}

/// <summary>Reads and controls the media session Windows shows in its own volume flyout.</summary>
internal static class MediaController
{
    private static GlobalSystemMediaTransportControlsSessionManager? _manager;
    private static string? _artKey;
    private static ImageSource? _art;
    private static int _artTries;

    private static async Task<GlobalSystemMediaTransportControlsSession?> SessionAsync()
    {
        _manager ??= await GlobalSystemMediaTransportControlsSessionManager.RequestAsync();
        return _manager.GetCurrentSession();
    }

    public static async Task<MediaInfo> GetAsync()
    {
        try
        {
            var session = await SessionAsync();
            if (session == null) return MediaInfo.None;

            var props = await session.TryGetMediaPropertiesAsync();
            bool playing = session.GetPlaybackInfo().PlaybackStatus
                           == GlobalSystemMediaTransportControlsSessionPlaybackStatus.Playing;

            // Album art: load once per song. Players often send it a moment after the song
            // starts, so keep trying for a few refreshes until it arrives.
            string key = props.Title + "|" + props.Artist;
            if (key != _artKey)
            {
                _artKey = key;
                _art = null;
                _artTries = 0;
            }
            if (_art == null && _artTries < 8)
            {
                _artTries++;
                _art = await LoadArtAsync(props.Thumbnail);
            }
            return new MediaInfo(true, props.Title ?? "", props.Artist ?? "", playing, _art,
                                 session.SourceAppUserModelId ?? "");
        }
        catch
        {
            return MediaInfo.None;
        }
    }

    public static Task PlayPauseAsync() => RunAsync(s => s.TryTogglePlayPauseAsync().AsTask());
    public static Task NextAsync() => RunAsync(s => s.TrySkipNextAsync().AsTask());
    public static Task PreviousAsync() => RunAsync(s => s.TrySkipPreviousAsync().AsTask());

    private static async Task RunAsync(Func<GlobalSystemMediaTransportControlsSession, Task<bool>> action)
    {
        try
        {
            if (await SessionAsync() is { } session) await action(session);
        }
        catch
        {
            // the player closed or refused — nothing to do
        }
    }

    private static async Task<ImageSource?> LoadArtAsync(IRandomAccessStreamReference? thumbnail)
    {
        if (thumbnail == null) return null;
        try
        {
            using var winrtStream = await thumbnail.OpenReadAsync();
            using var stream = winrtStream.AsStreamForRead();
            var memory = new MemoryStream();
            await stream.CopyToAsync(memory);
            memory.Position = 0;

            var bmp = new BitmapImage();
            bmp.BeginInit();
            bmp.CacheOption = BitmapCacheOption.OnLoad;
            bmp.DecodePixelWidth = 128;
            bmp.StreamSource = memory;
            bmp.EndInit();
            bmp.Freeze();
            return bmp;
        }
        catch
        {
            return null;
        }
    }
}
