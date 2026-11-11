use super::ranked_bar_runtime::{fixture, request};
use super::*;

#[test]
fn ranked_other_commits_and_reads_same_model_with_one_shape() -> Result<(), String> {
    let sink = IngestSink::default();
    publish_sizes(&sink, id(2), id(3), [25, 75])?;
    let f = fixture(sink)?;
    let view = f.open()?;
    let make = |seq| {
        let mut r = request(&view, seq);
        r["viewport"]["width"] = json!(4);
        r["viewport"]["height"] = json!(4);
        r
    };
    let physical = job(&f.ok("viz.render", make(1))?)?;
    f.entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    f.release.send(()).map_err(|e| e.to_string())?;
    f.finished(&physical, JobState::Done)?;
    let response = f.ok("viz.render", make(2))?;
    assert_eq!(response["kind"], "model");
    assert_eq!(response["resultRevision"], json!(id(3)));
    assert_eq!(
        response["model"]["shapes"]
            .as_array()
            .ok_or("shapes")?
            .len(),
        1
    );
    assert_eq!(response["model"]["shapes"][0]["groupRef"], "fixture.other");
    let model_ref = response["model"]["modelRef"].as_str().ok_or("model")?;
    let rows = f.ok(
        "viz.rows",
        json!({"modelRef":model_ref,"resultRevision":id(3),"start":0,"count":10}),
    )?;
    assert_eq!(rows["totalCount"], 1);
    assert_eq!(rows["rows"][0]["groupRef"], "fixture.other");
    let group = f
        .session
        .group_rows(model_ref, "fixture.other", 0, 10)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(group.result_revision, id(3));
    assert_eq!(group.rows.len(), 2);
    assert_eq!(
        group.group_total.amount,
        SizeAmount::Exact {
            bytes: 100_u64.into()
        }
    );
    let frame = f
        .session
        .ranked_frame(model_ref)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(frame.descriptor.strategy.revision, 1);
    assert_eq!(frame.shapes.len(), 1);
    Ok(())
}

#[test]
fn ranked_other_and_top_share_revision_and_cancel_or_replacement_withdraw() -> Result<(), String> {
    for mode in ["done", "cancel", "replace"] {
        let sink = IngestSink::default();
        let lineage = sink
            .bind_current_analysis("scope", id(1))
            .map_err(|e| format!("{e:?}"))?;
        let root = ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(b"/fixture"),
            },
            volume_id: vec![1],
            file_id: vec![2],
        };
        let mut build = sink
            .open_forest_in_lineage(id(2), vec![root], lineage)
            .map_err(|e| format!("{e:?}"))?;
        build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
        for (name, bytes) in [(b'a', 100_u64), (b'b', 50), (b'c', 25)] {
            build
                .add_file(Observation {
                    path: vec![vec![name]],
                    size: Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount: SizeAmount::Exact {
                            bytes: bytes.into(),
                        },
                    }),
                })
                .map_err(|e| format!("{e:?}"))?;
        }
        build
            .seal(id(3), ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        let f = fixture(sink)?;
        let view = f.open()?;
        let make = |seq| {
            let mut r = request(&view, seq);
            r["viewport"]["width"] = json!(8);
            r["viewport"]["height"] = json!(4);
            r
        };
        let physical = job(&f.ok("viz.render", make(1))?)?;
        f.entered
            .recv_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        if mode == "cancel" {
            assert_eq!(f.detach(&view, 1)?["controlResult"], "accepted");
        }
        if mode == "replace" {
            publish(&f.sink, id(2), id(4))?;
        }
        f.release.send(()).map_err(|e| e.to_string())?;
        f.finished(
            &physical,
            match mode {
                "done" => JobState::Done,
                "cancel" => JobState::Cancelled,
                _ => JobState::Failed,
            },
        )?;
        if mode != "done" {
            continue;
        }
        let response = f.ok("viz.render", make(2))?;
        assert_eq!(response["kind"], "model");
        assert_eq!(response["resultRevision"], json!(id(3)));
        assert_eq!(
            response["model"]["shapes"]
                .as_array()
                .ok_or("shapes")?
                .len(),
            2
        );
        assert_eq!(response["model"]["shapes"][1]["groupRef"], "fixture.other");
        let model_ref = response["model"]["modelRef"].as_str().ok_or("model")?;
        let rows = f.ok(
            "viz.rows",
            json!({"modelRef":model_ref,"resultRevision":id(3),"start":0,"count":10}),
        )?;
        assert_eq!(rows["rows"].as_array().ok_or("rows")?.len(), 2);
        assert_eq!(rows["rows"][1]["groupRef"], "fixture.other");
        let group = f
            .session
            .group_rows(model_ref, "fixture.other", 0, 10)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(group.result_revision, id(3));
        assert_eq!(group.rows.len(), 2);
        assert_eq!(
            group.group_total.amount,
            SizeAmount::Exact {
                bytes: 75_u64.into()
            }
        );
    }
    Ok(())
}
