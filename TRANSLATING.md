# Translating Circle

Circle is translated through [Fluent](https://projectfluent.org/) catalogues in
[`i18n/`](i18n/), one directory per locale, each holding a `circle.ftl` named
after the crate. That layout is what Hosted Weblate expects, which is why it is
not improvised locally.

Two languages ship today: `en` (the source, and the fallback for anything a
translation is missing) and `el`.

## Adding a language

Copy `i18n/en/circle.ftl` to `i18n/<locale>/circle.ftl` and translate the
values. Nothing else changes — no code, no build file, no list of locales
anywhere. The desktop entry, the applications-menu name, and the AppStream
summary translate along with the interface, because [`build.rs`](build.rs)
feeds the same catalogue to `xdgen`.

`just test` fails if a catalogue drifts: a message defined twice, one the
application no longer uses, or one missing from a translation. Run it before
opening a pull request — it is faster than a review round trip.

## Four things that bite

1. **Plurals go through Fluent, never through Rust.** A selector picks the
   form, and the rules differ per language in ways `if count == 1` cannot
   express. Keep the `{ $count -> }` block; do not replace it with one string.

   ```fluent
   selected-count = { $count ->
           [one] { $count } selected
          *[other] { $count } selected
       }
   ```

   Greek and English happen to agree here. Polish, Russian, and Arabic do not,
   and they need the extra arms (`few`, `many`, `zero`) their rules define.

2. **Keep every placeable.** `{ $name }`, `{ $count }`, `{ $why }` are filled
   in at runtime. A translation that drops one loses information the user
   needed; one that invents a name that was never passed renders the id
   instead of the value.

3. **Do not translate what is written to disk.** Nothing in these catalogues
   is stored, but the rule matters if you add a string: collection names, file
   names, and log lines stay in one language. A translated log line is harder
   to grep, not easier.

4. **The three `app-*` ids are not ordinary strings.** `app-title`,
   `app-comment` and `app-keywords` become the desktop entry and the AppStream
   metadata. `app-keywords` is a **semicolon-separated list** and must keep its
   trailing semicolon; each entry is a word somebody might type into a
   launcher, so translate for search rather than literally.

## What is not translated

`app-title` stays "Circle" in every language. It is the application's name,
not a description of it.
