using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace RogControl;

/// <summary>
/// Transparent resize handle that shows the appropriate resize cursor
/// (horizontal / vertical / diagonal) on hover. UIElement.ProtectedCursor is
/// protected, so a small subclass is the clean way to set it from XAML.
/// Panel hit-tests directly on its own Background (no template involved),
/// which keeps the invisible handle reliable.
/// </summary>
public sealed class ResizeThumb : Panel
{
    public static readonly DependencyProperty CursorShapeProperty =
        DependencyProperty.Register(
            nameof(CursorShape),
            typeof(InputSystemCursorShape),
            typeof(ResizeThumb),
            new PropertyMetadata(InputSystemCursorShape.Arrow, OnCursorShapeChanged));

    public InputSystemCursorShape CursorShape
    {
        get => (InputSystemCursorShape)GetValue(CursorShapeProperty);
        set => SetValue(CursorShapeProperty, value);
    }

    private static void OnCursorShapeChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        ((ResizeThumb)d).UpdateCursor();
    }

    private void UpdateCursor()
    {
        // Cache one cursor per shape to avoid leaking GDI handles on every change.
        ProtectedCursor = s_cursors.GetOrAdd(CursorShape, InputSystemCursor.Create);
    }

    private static readonly System.Collections.Concurrent.ConcurrentDictionary<InputSystemCursorShape, InputCursor> s_cursors = new();
}
