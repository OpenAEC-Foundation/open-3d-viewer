# IDS files of the built-in rule sets

The information delivery specifications the BIM Collaborator offers as rule sets of its own.
They come from the OpenAEC BIM Validator (MIT licence, OpenAEC Foundation and 3BM Bouwkunde),
`ids-bestanden/`, as they are but for one change: each specification also names `IFC4X3_ADD2`
in its `ifcVersion`, so models in IFC 4.3 are checked too. The entities and properties they
ask for are the same in IFC 4 and IFC 4.3.

| File | What it checks |
| --- | --- |
| `NL_BIM_Basis_ILS_v2.ids` | The Dutch BIM basis ILS: storeys, proxies, NL-SfB, load bearing, external, materials |
| `RVB_BIM_Norm_v1.1.ids` | The building information model norm v1.1 of the Rijksvastgoedbedrijf |
