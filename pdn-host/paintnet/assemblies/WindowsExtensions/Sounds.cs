// Windows' sounds. paint.rs plays none: a plug-in's sound loads and plays as silence, and its events still fire.
using System;
using System.ComponentModel;
using System.IO;

namespace System.Media;

public class SoundPlayer : Component
{
    private string soundLocation = "";
    private Stream? stream;

    public SoundPlayer() { }
    public SoundPlayer(string soundLocation) => this.soundLocation = soundLocation ?? "";
    public SoundPlayer(Stream? stream) => this.stream = stream;

    public string SoundLocation
    {
        get => soundLocation;
        set { soundLocation = value ?? ""; OnSoundLocationChanged(EventArgs.Empty); }
    }
    public Stream? Stream
    {
        get => stream;
        set { stream = value; OnStreamChanged(EventArgs.Empty); }
    }
    public int LoadTimeout { get; set; } = 10000;
    public bool IsLoadCompleted { get; private set; }
    public object? Tag { get; set; }

    public event AsyncCompletedEventHandler? LoadCompleted;
    public event EventHandler? SoundLocationChanged, StreamChanged;

    public void Load() => IsLoadCompleted = true;
    public void LoadAsync() { Load(); OnLoadCompleted(new AsyncCompletedEventArgs(null, false, null)); }
    public void Play() { }
    public void PlaySync() { }
    public void PlayLooping() { }
    public void Stop() { }

    protected virtual void OnLoadCompleted(AsyncCompletedEventArgs e) => LoadCompleted?.Invoke(this, e);
    protected virtual void OnSoundLocationChanged(EventArgs e) => SoundLocationChanged?.Invoke(this, e);
    protected virtual void OnStreamChanged(EventArgs e) => StreamChanged?.Invoke(this, e);
}

public class SystemSound
{
    internal SystemSound() { }
    public void Play() { }
}

public static class SystemSounds
{
    public static SystemSound Asterisk { get; } = new();
    public static SystemSound Beep { get; } = new();
    public static SystemSound Exclamation { get; } = new();
    public static SystemSound Hand { get; } = new();
    public static SystemSound Question { get; } = new();
}
