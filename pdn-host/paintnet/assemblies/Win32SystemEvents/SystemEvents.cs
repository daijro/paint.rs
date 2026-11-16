// Windows' system broadcasts (a colour scheme, font or display change, the session ending). Headless nothing
// broadcasts: handlers are kept and never called. Enum values are Windows'.
using System;

namespace Microsoft.Win32;

public static class SystemEvents
{
    public static event EventHandler? DisplaySettingsChanged, DisplaySettingsChanging, EventsThreadShutdown, InstalledFontsChanged, PaletteChanged, TimeChanged;
    [Obsolete] public static event EventHandler? LowMemory;
    public static event PowerModeChangedEventHandler? PowerModeChanged;
    public static event SessionEndedEventHandler? SessionEnded;
    public static event SessionEndingEventHandler? SessionEnding;
    public static event SessionSwitchEventHandler? SessionSwitch;
    public static event TimerElapsedEventHandler? TimerElapsed;
    public static event UserPreferenceChangedEventHandler? UserPreferenceChanged;
    public static event UserPreferenceChangingEventHandler? UserPreferenceChanging;
    public static IntPtr CreateTimer(int interval) => IntPtr.Zero;
    public static void KillTimer(IntPtr timerId) { }
    public static void InvokeOnEventsThread(Delegate method) => method.DynamicInvoke();
}

public enum UserPreferenceCategory { Accessibility = 1, Color, Desktop, General, Icon, Keyboard, Menu, Mouse, Policy, Power, Screensaver, Window, Locale, VisualStyle }
public enum PowerModes { Resume = 1, StatusChange, Suspend }
public enum SessionEndReasons { Logoff = 1, SystemShutdown }
public enum SessionSwitchReason { ConsoleConnect = 1, ConsoleDisconnect, RemoteConnect, RemoteDisconnect, SessionLogon, SessionLogoff, SessionLock, SessionUnlock, SessionRemoteControl }

public delegate void UserPreferenceChangedEventHandler(object sender, UserPreferenceChangedEventArgs e);
public delegate void UserPreferenceChangingEventHandler(object sender, UserPreferenceChangingEventArgs e);
public delegate void PowerModeChangedEventHandler(object sender, PowerModeChangedEventArgs e);
public delegate void SessionEndedEventHandler(object sender, SessionEndedEventArgs e);
public delegate void SessionEndingEventHandler(object sender, SessionEndingEventArgs e);
public delegate void SessionSwitchEventHandler(object sender, SessionSwitchEventArgs e);
public delegate void TimerElapsedEventHandler(object sender, TimerElapsedEventArgs e);

public class UserPreferenceChangedEventArgs(UserPreferenceCategory category) : EventArgs { public UserPreferenceCategory Category { get; } = category; }
public class UserPreferenceChangingEventArgs(UserPreferenceCategory category) : EventArgs { public UserPreferenceCategory Category { get; } = category; }
public class PowerModeChangedEventArgs(PowerModes mode) : EventArgs { public PowerModes Mode { get; } = mode; }
public class SessionEndedEventArgs(SessionEndReasons reason) : EventArgs { public SessionEndReasons Reason { get; } = reason; }
public class SessionEndingEventArgs(SessionEndReasons reason) : EventArgs { public SessionEndReasons Reason { get; } = reason; public bool Cancel { get; set; } }
public class SessionSwitchEventArgs(SessionSwitchReason reason) : EventArgs { public SessionSwitchReason Reason { get; } = reason; }
public class TimerElapsedEventArgs(IntPtr timerId) : EventArgs { public IntPtr TimerId { get; } = timerId; }
