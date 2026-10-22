using Jupiter.ChesscomCrawler.App.Services;
using Jupiter.ChesscomCrawler.App.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Jupiter.ChesscomCrawler.App.Views;

public sealed partial class SettingsView : UserControl
{
    private bool _syncing;

    public SettingsView()
    {
        ViewModel = App.Current.Settings;
        InitializeComponent();
        Loaded += (_, _) => SyncShellControls();
    }

    public SettingsViewModel ViewModel { get; }

    private AccentService Accent => App.Current.Accent;

    /// <summary>Shows the current sign-in and accent state.</summary>
    public void SyncShellControls()
    {
        _syncing = true;
        try
        {
            SignInSwitch.IsOn = StartupRegistration.IsEnabled;
            AccentChoice.SelectedIndex = Accent.UseCustomAccent ? 1 : 0;
            AccentSwatch.Background = new SolidColorBrush(Accent.Current);
            AccentPicker.Color = Accent.CustomColor;
        }
        finally
        {
            _syncing = false;
        }
    }

    private async void OnBrowseOutput(object sender, RoutedEventArgs e)
    {
        var picker = new Microsoft.Windows.Storage.Pickers.FolderPicker(App.Current.Window.AppWindow.Id);
        var folder = await picker.PickSingleFolderAsync();
        if (folder is not null)
        {
            ViewModel.OutputFolder = folder.Path;
        }
    }

    private void OnSignInToggled(object sender, RoutedEventArgs e)
    {
        if (_syncing)
        {
            return;
        }
        try
        {
            StartupRegistration.Set(SignInSwitch.IsOn);
        }
        catch (Exception ex) when (ex is UnauthorizedAccessException or System.Security.SecurityException or IOException)
        {
            SyncShellControls();
            App.Current.Status.AddEvents([new Messages.Event { Level = Messages.EventLevel.Error, Message = $"Changing the sign-in entry failed: {ex.Message}", UnixMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() }]);
        }
    }

    private void OnAccentChoiceChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_syncing)
        {
            return;
        }
        if (AccentChoice.SelectedIndex == 0)
        {
            Accent.UseWindows();
        }
        else
        {
            Accent.UseCustom(Accent.CustomColor);
        }
        SyncShellControls();
    }

    private void OnApplyCustomAccent(object sender, RoutedEventArgs e)
    {
        Accent.UseCustom(AccentPicker.Color);
        AccentSwatchButton.Flyout?.Hide();
        SyncShellControls();
    }

    private void OnResetAccent(object sender, RoutedEventArgs e)
    {
        Accent.UseWindows();
        SyncShellControls();
    }
}
