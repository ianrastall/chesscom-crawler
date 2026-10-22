using Jupiter.ChesscomCrawler.App.ViewModels;
using Microsoft.UI.Xaml.Controls;

namespace Jupiter.ChesscomCrawler.App.Views;

public sealed partial class StatusView : UserControl
{
    public StatusView()
    {
        ViewModel = App.Current.Status;
        InitializeComponent();
    }

    public StatusViewModel ViewModel { get; }
}
