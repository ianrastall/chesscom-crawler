using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Windows.System;

namespace Jupiter.ChesscomCrawler.App.Controls;

/// <summary>
/// A text box for a number, used instead of the WinUI number box (whose spin pop-up has no evident
/// way to close). It accepts digits only, plus one decimal separator when
/// <see cref="DecimalPlaces"/> is above zero and a leading minus when <see cref="Minimum"/> is negative.
/// The value applies as it is typed; leaving the field or pressing Enter brings it into range. An
/// empty box is <see cref="double.NaN"/>.
/// </summary>
public sealed partial class NumericTextBox : TextBox
{
    public static readonly DependencyProperty ValueProperty = DependencyProperty.Register(
        nameof(Value), typeof(double), typeof(NumericTextBox), new PropertyMetadata(double.NaN, OnValueChanged));

    private bool _updating;

    public NumericTextBox()
    {
        InputScope = new InputScope { Names = { new InputScopeName(InputScopeNameValue.Number) } };
        BeforeTextChanging += OnBeforeTextChanging;
        TextChanged += OnTextChanged;
        LostFocus += (_, _) => Normalize();
        KeyDown += OnKeyDown;
    }

    public double Value
    {
        get => (double)GetValue(ValueProperty);
        set => SetValue(ValueProperty, value);
    }

    public double Minimum { get; set; } = 0;

    public double Maximum { get; set; } = int.MaxValue;

    /// <summary>Digits allowed after the decimal separator; 0 means whole numbers only.</summary>
    public int DecimalPlaces { get; set; } = 0;

    private static string Separator => CultureInfo.CurrentCulture.NumberFormat.NumberDecimalSeparator;

    private string Format(double value) =>
        double.IsNaN(value) ? "" : Math.Round(value, DecimalPlaces).ToString(DecimalPlaces == 0 ? "0" : "0." + new string('#', DecimalPlaces), CultureInfo.CurrentCulture);

    private static void OnValueChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        var box = (NumericTextBox)d;
        if (box._updating)
        {
            return;
        }
        box._updating = true;
        box.Text = box.Format((double)e.NewValue);
        box._updating = false;
    }

    private void OnBeforeTextChanging(TextBox sender, TextBoxBeforeTextChangingEventArgs args)
    {
        var text = args.NewText;
        var start = Minimum < 0 && text.StartsWith('-') ? 1 : 0;
        var separator = DecimalPlaces > 0 ? text.IndexOf(Separator, start, StringComparison.Ordinal) : -1;
        for (var i = start; i < text.Length; i++)
        {
            if (separator >= 0 && i >= separator && i < separator + Separator.Length)
            {
                continue;
            }
            if (!char.IsAsciiDigit(text[i]))
            {
                args.Cancel = true;
                return;
            }
        }
        if (separator >= 0 && text.Length - separator - Separator.Length > DecimalPlaces)
        {
            args.Cancel = true;
        }
    }

    private void OnTextChanged(object sender, TextChangedEventArgs e)
    {
        if (_updating)
        {
            return;
        }
        double value;
        if (Text.Length == 0)
        {
            value = double.NaN;
        }
        else if (!TryParse(out var n) || n < Minimum || n > Maximum)
        {
            // Out of range or a lone minus: wait until the field is left.
            return;
        }
        else
        {
            value = n;
        }
        _updating = true;
        Value = value;
        _updating = false;
    }

    private void OnKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Enter)
        {
            Normalize();
        }
    }

    private bool TryParse(out double value) =>
        double.TryParse(Text, NumberStyles.AllowLeadingSign | NumberStyles.AllowDecimalPoint, CultureInfo.CurrentCulture, out value);

    /// <summary>Brings the text into range, or clears it when it holds no number.</summary>
    private void Normalize()
    {
        double value = double.NaN;
        if (TryParse(out var n))
        {
            value = Math.Clamp(Math.Round(n, DecimalPlaces), Minimum, Maximum);
        }
        _updating = true;
        Value = value;
        Text = Format(value);
        _updating = false;
    }
}
