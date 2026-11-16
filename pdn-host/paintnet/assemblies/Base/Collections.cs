// PaintDotNet.Collections helpers that pre-5.2 plugins call from the PaintDotNet.Base assembly
// (e.g. to build an enum dropdown's choices).
using System;
using System.Collections.Generic;
using System.Linq;

namespace PaintDotNet.Collections;

public static class EnumerableExtensions
{
    public static T[] ToArrayEx<T>(this IEnumerable<T> source) => source.ToArray();
    public static int IndexOf<T>(this IEnumerable<T> source, T value)
    {
        int i = 0;
        foreach (T item in source)
        {
            if (EqualityComparer<T>.Default.Equals(item, value)) return i;
            i++;
        }
        return -1;
    }
}

public static class ListExtensions
{
    public static IList<TResult> Select<T, TResult>(this IList<T> source, Func<T, TResult> selector)
        => Enumerable.Select(source, selector).ToList();
}
