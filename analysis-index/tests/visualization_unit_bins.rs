use du_analysis_index::visualization::{
    geometry::{Rect, Scale, Weight},
    histogram::{self, Basis, Bin, Item},
    unit_bins::{self, Error},
};
use du_core_contracts::{
    result::ResultCompleteness,
    size_units::{IEC_UNITS, SI_UNITS, UNIT_TABLE_REVISION},
};

#[test]
fn selected_tables_produce_only_unit_transitions_and_open_final_bin() -> Result<(), Error> {
    for table in [&SI_UNITS, &IEC_UNITS] {
        let result = unit_bins::from_unit_table(table, UNIT_TABLE_REVISION)?;
        let base = u64::from(table.base());
        let starts = [0, base, base.pow(2), base.pow(3), base.pow(4), base.pow(5)];
        assert_eq!(result.base, table.base());
        assert_eq!(result.table_revision, UNIT_TABLE_REVISION);
        assert_eq!(result.bins.len(), table.symbols().len());
        for (i, bin) in result.bins.iter().enumerate() {
            assert_eq!(
                *bin,
                Bin {
                    lower: starts[i],
                    upper: starts.get(i + 1).copied()
                }
            );
        }
    }
    assert_ne!(
        unit_bins::from_unit_table(&SI_UNITS, UNIT_TABLE_REVISION)?,
        unit_bins::from_unit_table(&IEC_UNITS, UNIT_TABLE_REVISION)?
    );
    assert_eq!(
        unit_bins::from_unit_table(&SI_UNITS, 0),
        Err(Error::UnsupportedRevision)
    );
    assert_eq!(
        unit_bins::from_unit_table(&SI_UNITS, UNIT_TABLE_REVISION + 1),
        Err(Error::UnsupportedRevision)
    );
    Ok(())
}

#[test]
fn generated_bins_preserve_zero_unknown_max_and_lower_bounds() -> Result<(), String> {
    for table in [&SI_UNITS, &IEC_UNITS] {
        let generated =
            unit_bins::from_unit_table(table, UNIT_TABLE_REVISION).map_err(|e| format!("{e:?}"))?;
        let base = u64::from(table.base());
        let last = base.pow(5);
        let input = [
            Item {
                item_no: 0,
                value: Weight::Exact(0),
            },
            Item {
                item_no: 1,
                value: Weight::Exact(base - 1),
            },
            Item {
                item_no: 2,
                value: Weight::Exact(base),
            },
            Item {
                item_no: 3,
                value: Weight::Exact(u64::MAX),
            },
            Item {
                item_no: 4,
                value: Weight::Unknown,
            },
            Item {
                item_no: 5,
                value: Weight::LowerBound(base),
            },
            Item {
                item_no: 6,
                value: Weight::LowerBound(last),
            },
        ];
        let output = histogram::layout(
            Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            &generated.bins,
            &input,
            Basis::ItemCount,
            Scale::Fixed(10),
            ResultCompleteness::Complete,
        )
        .map_err(|e| format!("{e:?}"))?;
        assert_eq!(output.bins[0].members, vec![0, 1]);
        assert_eq!(output.bins[1].members, vec![2]);
        assert_eq!(
            output.bins.last().ok_or("missing last bin")?.members,
            vec![3, 6]
        );
        assert_eq!(output.uncertain_items, vec![4, 5]);
        assert!(output.unclassified_items.is_empty());
    }
    Ok(())
}

#[test]
fn every_unit_transition_uses_the_next_half_open_bin() -> Result<(), String> {
    for table in [&SI_UNITS, &IEC_UNITS] {
        let bins = unit_bins::from_unit_table(table, UNIT_TABLE_REVISION)
            .map_err(|e| format!("{e:?}"))?
            .bins;
        let input: Vec<_> = bins
            .iter()
            .skip(1)
            .enumerate()
            .flat_map(|(i, bin)| {
                [
                    Item {
                        item_no: (2 * i) as u32,
                        value: Weight::Exact(bin.lower - 1),
                    },
                    Item {
                        item_no: (2 * i + 1) as u32,
                        value: Weight::Exact(bin.lower),
                    },
                ]
            })
            .collect();
        let output = histogram::layout(
            Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            &bins,
            &input,
            Basis::ItemCount,
            Scale::Fixed(10),
            ResultCompleteness::Complete,
        )
        .map_err(|e| format!("{e:?}"))?;
        for i in 0..bins.len() - 1 {
            assert!(output.bins[i].members.contains(&((2 * i) as u32)));
            assert!(output.bins[i + 1].members.contains(&((2 * i + 1) as u32)));
        }
    }
    Ok(())
}
