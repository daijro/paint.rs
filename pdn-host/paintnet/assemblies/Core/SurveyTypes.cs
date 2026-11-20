// An IPTC metadata item the survey corpus names (through a document's metadata). The host carries no IPTC store, so
// this exists as the type a plug-in reads off a metadata list; its key/value accessors are left to the member pass.
using System;
using System.Collections.Generic;

namespace PaintDotNet.Imaging
{
    public sealed class IptcPropertyItem
    {
        public IptcPropertyItem(IptcPropertyKey key, IptcValue value) { Key = key; Value = value; }
        public IptcPropertyKey Key { get; }
        public IptcValue Value { get; }
    }

    // IPTC key (record + dataset number) and value. The host keeps no IPTC store, so these are only the shapes a
    // plug-in reads off a metadata item (the list is empty on this path).
    public readonly struct IptcPropertyKey : IEquatable<IptcPropertyKey>
    {
        public IptcPropertyKey(string id) => ID = id;
        public string ID { get; }
        public bool Equals(IptcPropertyKey other) => ID == other.ID;
        public override bool Equals(object? obj) => obj is IptcPropertyKey o && Equals(o);
        public override int GetHashCode() => ID?.GetHashCode() ?? 0;
    }

    public sealed class IptcValue
    {
        public IptcValue(byte[] data) => Data = data;
        public IReadOnlyList<byte> Data { get; }
    }
}
