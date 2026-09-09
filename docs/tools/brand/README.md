# Brand assets

Everything under `static/images/` and `static/icons/` is generated. Do not hand-edit
those files: change the source or the script and re-run it.

```
./generate.sh
```

- `src/mark-source.jpg` is the single source illustration.
- `fonts/` holds Space Grotesk for rendering the wordmark. Upstream ships variable-only
  and its default instance is 300 (Light), so `fonts/build-static.sh` instantiates the
  Bold and Medium statics that ImageMagick needs. Both are OFL; see `fonts/OFL.txt`.
- Requires ImageMagick 7 (`magick`).

See `../../DESIGN.md` for why the icons rotate the jet, why the cut uses 14% fuzz, and
why the PNGs are quantised.
