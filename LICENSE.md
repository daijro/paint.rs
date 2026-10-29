# paint.rs licence

paint.rs — an open source, clean-room reimplementation of Paint.NET.
Copyright (C) 2026-present paint.rs contributors.

This program is free software: you can redistribute it and/or modify it under the terms of the
GNU General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without
even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
General Public License for more details.

You should have received a copy of the GNU General Public License along with this program in the
file `LICENSE`. If not, see <https://www.gnu.org/licenses/>.

SPDX-License-Identifier: GPL-3.0-or-later

## Third-party material under its own terms

paint.rs includes code adapted from paint.js (not its artwork), and through paint.js portions of
the Paint.NET 3.36 source. Those portions keep the notice below. Its MIT terms permit their
distribution as part of this GPL-licensed program; the notice must stay with them.

The three exceptions paint.js inherits from Paint.NET still govern the material they name, and the
GPL grants no rights in that material. This notably includes the interface text in
`assets/lang/en.json`, which derives from Paint.NET's resource strings through paint.js.

Other bundled third-party material keeps its own licence, listed in `NOTICE.md`: the fonts, the
codecs, the MobileSAM model (Apache-2.0, built into the desktop builds and served with the web
build; its licence in `assets/models/mobilesam/LICENSE`), and the code adapted from PhotoCraft,
PhotoSauce.MagicScaler, candle, TinyViT and paint.c (MIT; AA's Assistant's tap weights and alpha
curve).

---

## paint.js notice (the paint.js and Paint.NET 3.36 portions only; reproduced as published)

paint.js

Original Paint.NET portions Copyright (C) dotPDN LLC, Rick Brewster, Chris Crosetto, Tom Jackson,
Michael Kelsey, Brandon Ortiz, Craig Taylor, Chris Trevino, and Luke Walker.
Portions Copyright (C) Microsoft Corporation. All Rights Reserved.

JavaScript port and port-specific changes Copyright (C) 2024-present LabyStudio.

This software is licensed under the MIT License, with three (3) exceptions inherited from Paint.NET:

1. The Paint.NET logo and icon artwork are Copyright (C) Rick Brewster.
   Licensed under Creative Commons Attribution-NonCommercial-NoDerivs 2.5
   (http://creativecommons.org/licenses/by-nc-nd/2.5/). Usage is allowed when
   directly discussing or promoting Paint.NET (e.g., blog posts, "Made with Paint.NET" watermarks).

2. Paint.NET resource assets (toolbar graphics, menu text, status bar text, .RESOURCES/.RESX/.PNG files,
   and embedded resources in PaintDotNet.Resources.dll) are licensed under Creative Commons
   Attribution-NonCommercial-NoDerivs 2.5. Derivative works of these assets are allowed only for translation purposes.
   Some assets remain under their original licenses (e.g., Silk icons, Visual Studio 2005 Image Library).

3. GPC code included in Paint.NET may not be used in commercial applications without a
   GPC Commercial Use License from The University of Manchester (http://www.cs.man.ac.uk/~toby/alan/software/).

MIT License:

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction,
including the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, subject to the conditions above.

The above copyright notice and this permission notice shall be included in all copies or substantial
portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT.
