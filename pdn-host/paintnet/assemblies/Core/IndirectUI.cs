// PaintDotNet.IndirectUI: the config-dialog description an effect builds in OnCreateConfigUI.
// Headless, so nothing is ever turned into controls; the host reads the description back (display
// names, descriptions, choice labels) to label paint.rs's own dialog. The ControlInfo tree and its
// find/set logic are adapted from Kaint's src/pdnbridge (https://github.com/ssserch/Kaint,
// GPL-3.0), whose IndirectUI derives from Paint.NET 3.36 (MIT; Copyright (C) dotPDN LLC, Rick
// Brewster, Tom Jackson, and contributors — see NOTICE.md). Enum member order follows genuine
// Paint.NET 5.1.12, since plugins compile the values in as integers.
using System;
using System.Collections.Generic;
using PaintDotNet.PropertySystem;

namespace PaintDotNet.IndirectUI
{
    public enum ControlInfoPropertyNames
    {
        DisplayName, Description, ControlType, ButtonText, UseExponentialScale, DecimalPlaces,
        SliderSmallChange, SliderSmallChangeX, SliderSmallChangeY, SliderLargeChange, SliderLargeChangeX,
        SliderLargeChangeY, UpDownIncrement, UpDownIncrementX, UpDownIncrementY, StaticImageUnderlay,
        Multiline, ShowResetButton, SliderShowTickMarks, SliderShowTickMarksX, SliderShowTickMarksY,
        WindowTitle, WindowWidthScale, WindowIsSizable, SliderSmallChangeZ, SliderLargeChangeZ,
        UpDownIncrementZ, SliderShowTickMarksZ, Footnote, RangeWraps, ControlStyle, ControlColors,
        WindowHelpContentType, WindowHelpContent, AllowAllFiles, FileTypes, ShowHeaderLine,
        WindowShowBottomSeparatorLine, ExponentialScale,
    }

    public enum PropertyControlType
    {
        AngleChooser, CheckBox, PanAndSlider, Slider, IncrementButton, DropDown, TextBox, RadioButton,
        ColorWheel, RollBallAndSliders, FileChooser, LinkLabel, FolderChooser, Null, Label,
    }

    public enum WindowHelpContentType { None, PlainText, CustomViaCallback }

    public enum SliderControlStyle { Default, Hue, HueCentered, SaturationHue }

    public abstract class ControlInfo : ICloneable
    {
        private readonly List<ControlInfo> children = new();

        internal ControlInfo() { }
        internal ControlInfo(ControlInfo cloneMe)
        {
            foreach (ControlInfo c in cloneMe.children) children.Add(c.Clone());
        }

        public IReadOnlyList<ControlInfo> ChildControls => children.AsReadOnly();
        protected IList<ControlInfo> GetChildControlsCore() => children;

        /// One control per property, in order, in a panel (genuine's default dialog layout).
        public static ControlInfo CreateDefaultConfigUI(IEnumerable<Property> properties)
        {
            var panel = new PanelControlInfo();
            foreach (Property p in properties) panel.AddChildControl(PropertyControlInfo.CreateFor(p));
            return panel;
        }

        public PropertyControlInfo? FindControlForPropertyName(object propertyName)
        {
            if (this is PropertyControlInfo pci && pci.Property.Name == propertyName.ToString()) return pci;
            foreach (ControlInfo c in children)
                if (c.FindControlForPropertyName(propertyName) is { } found) return found;
            return null;
        }

        public bool SetPropertyControlValue(object propertyName, object controlPropertyName, object? propertyValue)
        {
            if (FindControlForPropertyName(propertyName) is not { } pci) return false;
            pci.SetControlValue(controlPropertyName, propertyValue);
            return true;
        }

        public bool SetPropertyControlType(object propertyName, PropertyControlType newControlType)
        {
            if (FindControlForPropertyName(propertyName) is not { } pci) return false;
            pci.ControlType.Value = newControlType;
            return true;
        }

        public abstract ControlInfo Clone();
        object ICloneable.Clone() => Clone();
    }

    public sealed class PanelControlInfo : ControlInfo
    {
        public PanelControlInfo() { }
        private PanelControlInfo(PanelControlInfo cloneMe) : base(cloneMe) { }
        public T AddChildControl<T>(T controlInfo) where T : ControlInfo { GetChildControlsCore().Add(controlInfo); return controlInfo; }
        public void RemoveChildControl(ControlInfo controlInfo) => GetChildControlsCore().Remove(controlInfo);
        public override ControlInfo Clone() => new PanelControlInfo(this);
    }

    public sealed class PropertyControlInfo : ControlInfo
    {
        private readonly Dictionary<object, string> valueDisplayNames;
        // Control properties by name (DisplayName, Description, SliderLargeChange, …); unset = default.
        private readonly Dictionary<string, object?> values;

        public Property Property { get; }
        public StaticListChoiceProperty ControlType { get; }

        private PropertyControlInfo(Property property)
        {
            Property = property;
            ControlType = StaticListChoiceProperty.CreateForEnum(typeof(PropertyControlType), ControlInfoPropertyNames.ControlType, DefaultControlType(property));
            valueDisplayNames = new();
            values = new(StringComparer.Ordinal);
        }

        private PropertyControlInfo(PropertyControlInfo cloneMe) : base(cloneMe)
        {
            Property = cloneMe.Property;
            ControlType = (StaticListChoiceProperty)cloneMe.ControlType.Clone();
            valueDisplayNames = new(cloneMe.valueDisplayNames);
            values = new(cloneMe.values, StringComparer.Ordinal);
        }

        private static PropertyControlType DefaultControlType(Property p) => p switch
        {
            BooleanProperty => PropertyControlType.CheckBox,
            StaticListChoiceProperty => PropertyControlType.DropDown,
            StringProperty => PropertyControlType.TextBox,
            _ => PropertyControlType.Slider,
        };

        public static PropertyControlInfo CreateFor(Property property) => new(property);

        public void SetValueDisplayName(object value, string displayName) => valueDisplayNames[value] = displayName;
        public string GetValueDisplayName(object value)
            => valueDisplayNames.TryGetValue(value, out string? name) ? name : value?.ToString() ?? "";

        internal void SetControlValue(object controlPropertyName, object? value) => values[controlPropertyName.ToString()!] = value;

        /// A control property the effect set (null if never set). Not in genuine Paint.NET, which
        /// exposes these through ControlProperties; the host reads labels with it.
        public object? GetControlValue(ControlInfoPropertyNames name) => values.GetValueOrDefault(name.ToString());

        public override ControlInfo Clone() => new PropertyControlInfo(this);
    }

    public record struct TabContainerState(int SelectedTabIndex);

    /// The selected tab, stored with the effect's properties so a token remembers it.
    public sealed class TabContainerStateProperty : Property<TabContainerState>
    {
        public TabContainerStateProperty(object name) : base(name, default, false) { }
        public override Property Clone() => new TabContainerStateProperty(Name) { Value = Value };
    }

    public sealed class TabContainerControlInfo : ControlInfo
    {
        public Property? StateProperty { get; }
        public TabContainerControlInfo() { }
        public TabContainerControlInfo(Property? stateProperty) => StateProperty = stateProperty;
        public TabContainerControlInfo(TabContainerStateProperty stateProperty) => StateProperty = stateProperty;
        private TabContainerControlInfo(TabContainerControlInfo cloneMe) : base(cloneMe) => StateProperty = cloneMe.StateProperty;
        public TabPageControlInfo AddTab(TabPageControlInfo controlInfo) { GetChildControlsCore().Add(controlInfo); return controlInfo; }
        public override ControlInfo Clone() => new TabContainerControlInfo(this);
    }

    public sealed class TabPageControlInfo : ControlInfo
    {
        public string? Text { get; set; }
        public string? ToolTipText { get; set; }
        public TabPageControlInfo() { }
        private TabPageControlInfo(TabPageControlInfo cloneMe) : base(cloneMe) { Text = cloneMe.Text; ToolTipText = cloneMe.ToolTipText; }
        public T AddChildControl<T>(T controlInfo) where T : ControlInfo { GetChildControlsCore().Add(controlInfo); return controlInfo; }
        public override ControlInfo Clone() => new TabPageControlInfo(this);
    }
}
