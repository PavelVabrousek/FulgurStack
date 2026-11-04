# Built-in deep-sky table

`deep_sky.csv` is built from the OpenNGC database by Mattia Verga and contributors (https://github.com/mattiaverga/OpenNGC, commit `be3cd6c`, files `NGC.csv` and `addendum.csv`). OpenNGC is licensed under the Creative Commons Attribution-ShareAlike 4.0 International licence (CC BY-SA 4.0); this derived table is shared under the same licence.

It also holds the complete Lynds Bright Nebulae (LBN), Sharpless (Sh2) and Barnard catalogues: 1 029 LBN objects are added (95 more were already OpenNGC aliases, 1 124 found in all), plus 313 Sh2 and 370 Barnard objects; 15 739 entries in total. Their positions come from SIMBAD (operated at CDS, Strasbourg, France; queried through its CfA mirror, `https://simbad.cfa.harvard.edu/simbad/sim-tap/sync`, on 2026-11-04) with:

```sql
SELECT i.id, b.ra, b.dec, b.main_id FROM ident AS i JOIN basic AS b ON i.oidref = b.oid WHERE i.id LIKE 'LBN %'
```

and the same query with `'SH  2-%'` and `'Barnard %'`. Only plain catalogue numbers are kept (`LBN 437`, `Sh2-155`, `B 33`). SIMBAD's common names (`NAME …`) become aliases. A name OpenNGC already has keeps OpenNGC's entry. Use of SIMBAD data asks for the acknowledgement "This research has made use of the SIMBAD database, operated at CDS, Strasbourg, France".

Rebuild it with:

```bash
cargo run --release -p fulgur-catalog --example build_deep_sky -- NGC.csv addendum.csv crates/fulgur-catalog/data/deep_sky.csv simbad_lbn.csv simbad_sh2.csv simbad_barnard.csv
```

Each line holds `NAME;RA_deg;DEC_deg;ALIASES`. The aliases (separated by `|`) are:
- the Messier number;
- the common names;
- the identifiers in the catalogues observers name targets by: Caldwell, LBN, LDN, Sh2, Barnard, Melotte, Collinder, vdB, Ced, Abell.

Duplicate NGC/IC entries take the position of the object they repeat.
