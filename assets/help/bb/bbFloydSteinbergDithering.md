# Floyd-Steinberg Dithering

Dithering represents colors with a limited palette. Each pixel takes its nearest palette color, and its rounding error spreads to pixels that have not yet been processed. See the [encyclopedia article](https://en.wikipedia.org/wiki/Floyd%E2%80%93Steinberg_dithering) for background.

Choose black and white for a dotted monochrome image, four or eight grays for tonal steps, CMYK for process-like colors, either 16-color palette for a retro look, or Pebble Time for 64 colors. Plus One uses your Custom Color; Current Palette uses the chosen number of your swatches.

Floyd-Steinberg spreads error right by 7/16, then down-left, down and down-right by 3/16, 5/16 and 1/16.

The Custom matrix sends 8/32 and 4/32 rightward; the next row receives 4/32, 8/32, 4/32 and 1/32 from down-left across; the following row receives 2/32 directly below and 1/32 below-right. Compare both methods on a smooth gradient.

For questions about the original effects, visit the [Paint.NET forum](https://forums.getpaint.net/).
