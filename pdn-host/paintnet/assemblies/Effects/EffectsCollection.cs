// SubtleEffect builds this over the assemblies it wants effects from. ponytail: inert — the host registers effects
// through its own path, so Effects is empty; real = scan the assemblies for the effect base type.
using System;
using System.Collections.Generic;
using System.Reflection;

namespace PaintDotNet.Effects;

public sealed class EffectsCollection
{
    public EffectsCollection(List<Assembly> assemblies) { }
    public Type[] Effects => Array.Empty<Type>();
}
