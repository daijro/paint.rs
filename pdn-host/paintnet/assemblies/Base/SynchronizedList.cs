// PaintDotNet.Collections.SynchronizedList<T>: a lock-wrapped List<T> that classic plug-ins
// (pyrochild's ScriptLab keeps its background render's exceptions in one) use as a thread-safe list.
using System.Collections;
using System.Collections.Generic;

namespace PaintDotNet.Collections;

public sealed class SynchronizedList<T> : IList<T>
{
    private readonly IList<T> list;
    private readonly object sync = new();

    public SynchronizedList() => list = new List<T>();
    public SynchronizedList(int capacity) => list = new List<T>(capacity);
    public SynchronizedList(IEnumerable<T> collection) => list = new List<T>(collection);
    /// A synchronized view of `list` itself (not a copy), as Paint.NET's; `sync` is the lock its members take.
    public SynchronizedList(IList<T> list) => this.list = list;
    public SynchronizedList(IList<T> list, System.Threading.Lock sync) : this(list) => this.sync = sync;

    public object SyncRoot => sync;

    /// A snapshot of the items, as PDN's SynchronizedList hands out (callers enumerate it without holding the lock).
    public IList<T> Items { get { lock (sync) return System.Linq.Enumerable.ToArray(list); } }

    public int Count { get { lock (sync) return list.Count; } }
    public bool IsReadOnly => false;

    public T this[int index]
    {
        get { lock (sync) return list[index]; }
        set { lock (sync) list[index] = value; }
    }

    public void Add(T item) { lock (sync) list.Add(item); }
    public void AddRange(IEnumerable<T> items) { lock (sync) foreach (T item in items) list.Add(item); }
    public void Clear() { lock (sync) list.Clear(); }
    public bool Contains(T item) { lock (sync) return list.Contains(item); }
    public void CopyTo(T[] array, int arrayIndex) { lock (sync) list.CopyTo(array, arrayIndex); }
    public int IndexOf(T item) { lock (sync) return list.IndexOf(item); }
    public void Insert(int index, T item) { lock (sync) list.Insert(index, item); }
    public bool Remove(T item) { lock (sync) return list.Remove(item); }
    public void RemoveAt(int index) { lock (sync) list.RemoveAt(index); }
    public T[] ToArray() { lock (sync) return System.Linq.Enumerable.ToArray(list); }

    // A copy, so enumeration does not hold the lock or trip on a concurrent change (as PDN's does).
    public IEnumerator<T> GetEnumerator() { lock (sync) return new List<T>(list).GetEnumerator(); }
    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
}

/// PDN's two-parameter form, which keeps the backing list's type as a parameter (ScriptLab names it as well
/// as the one-parameter form). The backing type is unconstrained here since the corpus only names it; its
/// behaviour is a plain lock-wrapped list of T.
public sealed class SynchronizedList<TList, T> : IList<T>
{
    private readonly List<T> list = new();
    private readonly object sync = new();

    public object SyncRoot => sync;
    public int Count { get { lock (sync) return list.Count; } }
    public bool IsReadOnly => false;
    public T this[int index]
    {
        get { lock (sync) return list[index]; }
        set { lock (sync) list[index] = value; }
    }
    public void Add(T item) { lock (sync) list.Add(item); }
    public void Clear() { lock (sync) list.Clear(); }
    public bool Contains(T item) { lock (sync) return list.Contains(item); }
    public void CopyTo(T[] array, int arrayIndex) { lock (sync) list.CopyTo(array, arrayIndex); }
    public int IndexOf(T item) { lock (sync) return list.IndexOf(item); }
    public void Insert(int index, T item) { lock (sync) list.Insert(index, item); }
    public bool Remove(T item) { lock (sync) return list.Remove(item); }
    public void RemoveAt(int index) { lock (sync) list.RemoveAt(index); }
    public IEnumerator<T> GetEnumerator() { lock (sync) return new List<T>(list).GetEnumerator(); }
    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
}

/// PDN's chunked list for large element counts (it stores them in segments); a plain List<T> here, with the
/// same observable behaviour.
public sealed class SegmentedList<T> : IList<T>
{
    private readonly List<T> list;

    public SegmentedList() => list = new List<T>();
    public SegmentedList(int capacity) => list = new List<T>(capacity);
    public SegmentedList(IEnumerable<T> collection) => list = new List<T>(collection);

    public int Count => list.Count;
    public bool IsReadOnly => false;
    public T this[int index] { get => list[index]; set => list[index] = value; }
    public void Add(T item) => list.Add(item);
    public void AddRange(IEnumerable<T> items) => list.AddRange(items);
    public void Clear() => list.Clear();
    public bool Contains(T item) => list.Contains(item);
    public void CopyTo(T[] array, int arrayIndex) => list.CopyTo(array, arrayIndex);
    public int IndexOf(T item) => list.IndexOf(item);
    public void Insert(int index, T item) => list.Insert(index, item);
    public bool Remove(T item) => list.Remove(item);
    public void RemoveAt(int index) => list.RemoveAt(index);
    public IEnumerator<T> GetEnumerator() => list.GetEnumerator();
    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
}
