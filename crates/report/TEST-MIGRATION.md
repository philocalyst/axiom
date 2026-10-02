# Report source-test migrations

This note records the source-driven replacements for report tests whose inputs
used the removed v3 account chart or basis pseudo-place.

| Former test | Native replacement | Preserved evidence |
|---|---|---|
| `flow_recognizes_a_spread_premium_a_little_each_day` | `source_tests::native_annual_premium_is_spread_across_months` and `source_tests::native_sales_report_realized_short_and_long_gains` | The $1,200 annual premium still contributes $101.92, $92.05, and $101.92 in January through March ($295.89 to date). Realized short- and long-term gains remain $400 in February and $800 in March ($1,200 total), now through the native Gains view. |
| `a_monthly_depreciation_does_not_end_because_the_house_holds_no_dollars` | `source_tests::native_asset_law_forecast_changes_basis_without_moving_cash` | v4 records depreciation as an asset law consuming basis, not as a journal transfer from `house.basis`. The native fixture asserts $600 consumed through February, $3,000 more through December, a $900 annual recapture, and unchanged checking cash from depreciation. |

These replacements cover those specific behaviors; they do not claim that the
remaining report and engine migration obligations are complete.
