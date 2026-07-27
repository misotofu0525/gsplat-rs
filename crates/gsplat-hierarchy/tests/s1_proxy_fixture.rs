use gsplat_hierarchy::{
    BuildConfig, CutError, DrawableGaussian, HierarchyError, LeafRange, NodeId,
    build_authored_proxy_hierarchy, build_formal_s1_proxy_hierarchy,
};

fn sh3_rest(seed: f32) -> [f32; 45] {
    std::array::from_fn(|index| seed + (index as f32 + 1.0) * 0.003125)
}

fn fixture_source() -> Vec<DrawableGaussian> {
    vec![
        DrawableGaussian {
            position: [-1.0, 0.0, 2.0],
            scale: [0.12, 0.09, 0.08],
            rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            opacity: 0.72,
            sh_dc: [0.8, 0.1, 0.2],
            sh_degree: 3,
            sh_rest: sh3_rest(-0.2),
        },
        DrawableGaussian {
            position: [-0.2, 0.3, 2.5],
            scale: [0.08, 0.11, 0.07],
            rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            opacity: 0.64,
            sh_dc: [0.2, 0.7, 0.3],
            sh_degree: 3,
            sh_rest: sh3_rest(0.1),
        },
        DrawableGaussian {
            position: [1.2, -0.1, 3.0],
            scale: [0.15, 0.13, 0.1],
            rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            opacity: 0.81,
            sh_dc: [0.1, 0.3, 0.9],
            sh_degree: 3,
            sh_rest: sh3_rest(0.4),
        },
    ]
}

fn formal_fixture_source(count: usize) -> Vec<DrawableGaussian> {
    let templates = fixture_source();
    (0..count)
        .map(|index| {
            let mut gaussian = templates[index % templates.len()];
            gaussian.position[0] += index as f32 * 0.75;
            gaussian.sh_rest[0] += index as f32 * 0.001;
            gaussian
        })
        .collect()
}

fn assert_f32_bits_equal(actual: f32, expected: f32, field: &str) {
    assert_eq!(actual.to_bits(), expected.to_bits(), "{field} changed bits");
}

fn assert_gaussian_bits_equal(actual: &DrawableGaussian, expected: &DrawableGaussian) {
    for (index, (actual, expected)) in actual
        .position
        .into_iter()
        .zip(expected.position)
        .enumerate()
    {
        assert_f32_bits_equal(actual, expected, &format!("position[{index}]"));
    }
    for (index, (actual, expected)) in actual.scale.into_iter().zip(expected.scale).enumerate() {
        assert_f32_bits_equal(actual, expected, &format!("scale[{index}]"));
    }
    for (index, (actual, expected)) in actual
        .rotation_xyzw
        .into_iter()
        .zip(expected.rotation_xyzw)
        .enumerate()
    {
        assert_f32_bits_equal(actual, expected, &format!("rotation_xyzw[{index}]"));
    }
    assert_f32_bits_equal(actual.opacity, expected.opacity, "opacity");
    for (index, (actual, expected)) in actual.sh_dc.into_iter().zip(expected.sh_dc).enumerate() {
        assert_f32_bits_equal(actual, expected, &format!("sh_dc[{index}]"));
    }
    assert_eq!(actual.sh_degree, expected.sh_degree, "sh_degree changed");
    for (index, (actual, expected)) in actual.sh_rest.into_iter().zip(expected.sh_rest).enumerate()
    {
        assert_f32_bits_equal(actual, expected, &format!("sh_rest[{index}]"));
    }
}

fn node_for_range(bundle: &gsplat_hierarchy::HierarchyBundle, start: u64, end: u64) -> NodeId {
    bundle
        .manifest
        .nodes
        .iter()
        .find(|node| node.leaf_range == LeafRange { start, end })
        .unwrap_or_else(|| panic!("missing node for range {start}..{end}"))
        .id
}

#[test]
fn deterministic_fixture_has_recursive_complete_coverage() {
    let source = fixture_source();
    let config = BuildConfig {
        source_leaves_per_node: 1,
    };
    let first = build_authored_proxy_hierarchy(&source, config).expect("first build");
    let second = build_authored_proxy_hierarchy(&source, config).expect("second build");

    assert_eq!(
        first.manifest.canonical_bytes(),
        second.manifest.canonical_bytes()
    );
    assert_eq!(
        first.manifest.content_hash(),
        second.manifest.content_hash()
    );
    assert_eq!(first.pages, second.pages);
    first.validate(&source).expect("valid authored hierarchy");

    let a1 = node_for_range(&first, 0, 1);
    let a2 = node_for_range(&first, 1, 2);
    let b = node_for_range(&first, 2, 3);
    let a = node_for_range(&first, 0, 2);
    let root = node_for_range(&first, 0, 3);

    // The recursive rule allows A to refine independently while B remains at
    // its own depth. All source ranges remain represented exactly once.
    first
        .manifest
        .validate_cut(&[a1, a2, b])
        .expect("{A1,A2,B} is a valid mixed-depth cut");
    first
        .manifest
        .validate_cut(&[a, b])
        .expect("{A,B} is a valid coarser cut");
    first
        .manifest
        .validate_cut(&[root])
        .expect("root is a complete bootstrap cut");

    assert_eq!(
        first.manifest.validate_cut(&[a1, b]),
        Err(CutError::MissingCoverage(a2)),
        "a missing sibling must leave its lineage uncovered"
    );
    assert_eq!(
        first.manifest.validate_cut(&[a, a1, b]),
        Err(CutError::AncestorDescendantOverlap {
            ancestor: a,
            descendant: a1,
        }),
        "an ancestor and descendant cannot both represent one lineage"
    );
}

#[test]
fn every_node_is_drawable_and_complete_leaf_cut_is_source_exact() {
    let source = fixture_source();
    let bundle = build_authored_proxy_hierarchy(
        &source,
        BuildConfig {
            source_leaves_per_node: 1,
        },
    )
    .expect("hierarchy");

    for node in &bundle.manifest.nodes {
        assert!(node.payload_splat_count > 0);
        assert!(node.geometric_error.is_finite());
        assert!(node.bounds_min.into_iter().all(f32::is_finite));
        assert!(node.bounds_max.into_iter().all(f32::is_finite));
    }

    let leaf_cut = bundle.manifest.leaf_cut();
    bundle
        .manifest
        .validate_cut(&leaf_cut)
        .expect("complete leaf cut");
    let decoded = bundle
        .materialize_cut(&source, &leaf_cut)
        .expect("validated leaf payloads");
    assert_eq!(decoded.len(), source.len());
    for (actual, expected) in decoded.iter().zip(&source) {
        assert_gaussian_bits_equal(actual, expected);
    }
}

#[test]
fn page_identity_changes_when_authored_payload_changes() {
    let source = fixture_source();
    let config = BuildConfig {
        source_leaves_per_node: 1,
    };
    let first = build_authored_proxy_hierarchy(&source, config).expect("first hierarchy");
    let mut changed = source;
    changed[1].sh_dc[2] = 0.875;
    let second = build_authored_proxy_hierarchy(&changed, config).expect("changed hierarchy");

    assert_ne!(
        first.manifest.content_hash(),
        second.manifest.content_hash()
    );
    assert_ne!(first.pages, second.pages);
    assert!(
        first
            .pages
            .iter()
            .all(|page| page.content_hash.hex().len() == 64)
    );
}

#[test]
fn overlapping_child_leaf_range_is_rejected() {
    let source = fixture_source();
    let mut bundle = build_authored_proxy_hierarchy(
        &source,
        BuildConfig {
            source_leaves_per_node: 1,
        },
    )
    .expect("hierarchy");
    let a2 = node_for_range(&bundle, 1, 2);
    bundle.manifest.nodes[a2.0 as usize].leaf_range = LeafRange { start: 0, end: 2 };

    assert!(matches!(
        bundle.validate(&source),
        Err(HierarchyError::Malformed(
            "children must partition the parent range with monotone error"
        ))
    ));
}

#[test]
fn materialize_rejects_tampered_page_before_decode() {
    let source = fixture_source();
    let mut bundle = build_authored_proxy_hierarchy(
        &source,
        BuildConfig {
            source_leaves_per_node: 1,
        },
    )
    .expect("hierarchy");
    let leaf_cut = bundle.manifest.leaf_cut();
    let tampered_node = bundle.pages[0].node;
    let last = bundle.pages[0].bytes.len() - 1;
    bundle.pages[0].bytes[last] ^= 0x01;

    assert_eq!(
        bundle.materialize_cut(&source, &leaf_cut),
        Err(HierarchyError::PageHashMismatch(tampered_node))
    );
}

#[test]
fn materialize_rejects_cycles_and_duplicate_roots_without_recursion() {
    let source = fixture_source();
    let config = BuildConfig {
        source_leaves_per_node: 1,
    };
    let mut cyclic = build_authored_proxy_hierarchy(&source, config).expect("cyclic fixture");
    let root = cyclic.manifest.roots[0];
    cyclic.manifest.nodes[root.0 as usize].children = vec![root];
    assert_eq!(
        cyclic.materialize_cut(&source, &[root]),
        Err(HierarchyError::Malformed("hierarchy contains a cycle"))
    );

    let mut duplicate_root =
        build_authored_proxy_hierarchy(&source, config).expect("duplicate-root fixture");
    let root = duplicate_root.manifest.roots[0];
    duplicate_root.manifest.roots.push(root);
    assert_eq!(
        duplicate_root.materialize_cut(&source, &[root]),
        Err(HierarchyError::Malformed("root ids must be unique"))
    );
}

#[test]
fn manifest_encoding_normalizes_root_and_page_order() {
    let source = fixture_source();
    let bundle = build_authored_proxy_hierarchy(
        &source,
        BuildConfig {
            source_leaves_per_node: 1,
        },
    )
    .expect("hierarchy");
    let root = bundle.manifest.roots[0];
    let a = node_for_range(&bundle, 0, 2);

    let mut forward = bundle.manifest.clone();
    forward.roots = vec![a, root];
    forward.pages.sort_by_key(|page| page.node);
    let mut reversed = forward.clone();
    reversed.roots.reverse();
    reversed.pages.reverse();

    assert_eq!(forward.canonical_bytes(), reversed.canonical_bytes());
    assert_eq!(forward.content_hash(), reversed.content_hash());
}

#[test]
fn formal_s1_builder_derives_frozen_cuts_before_images() {
    let source = formal_fixture_source(8);
    let (bundle, cuts) = build_formal_s1_proxy_hierarchy(
        &source,
        BuildConfig {
            source_leaves_per_node: 1,
        },
    )
    .expect("formal S1 hierarchy");

    assert_eq!(cuts.complete_leaf_exact, bundle.manifest.leaf_cut());
    assert_eq!(cuts.bootstrap_roots, bundle.manifest.roots);
    assert_eq!(cuts.bootstrap_roots.len(), 1);
    assert_eq!(cuts.mixed_depth_two_replacements.len(), 3);

    for cut in [
        &cuts.complete_leaf_exact,
        &cuts.bootstrap_roots,
        &cuts.mixed_depth_two_replacements,
    ] {
        bundle
            .manifest
            .validate_cut(cut)
            .expect("frozen cut retains complete recursive coverage");
        bundle
            .materialize_cut(&source, cut)
            .expect("frozen cut has valid content-addressed pages");
    }

    let mixed_ranges = cuts
        .mixed_depth_two_replacements
        .iter()
        .map(|id| bundle.manifest.node(*id).expect("mixed node").leaf_range)
        .collect::<Vec<_>>();
    assert_eq!(
        mixed_ranges,
        vec![
            LeafRange { start: 0, end: 2 },
            LeafRange { start: 2, end: 4 },
            LeafRange { start: 4, end: 8 },
        ]
    );
}

#[test]
fn formal_s1_builder_fails_closed_for_downgraded_sh_or_shallow_hierarchy() {
    let mut downgraded = formal_fixture_source(8);
    downgraded[3].sh_degree = 2;
    assert_eq!(
        build_formal_s1_proxy_hierarchy(
            &downgraded,
            BuildConfig {
                source_leaves_per_node: 1,
            }
        ),
        Err(HierarchyError::FormalSourceRequiresSh3 {
            actual: 2,
            index: 3,
        })
    );

    let shallow = formal_fixture_source(2);
    assert_eq!(
        build_formal_s1_proxy_hierarchy(
            &shallow,
            BuildConfig {
                source_leaves_per_node: 1,
            }
        ),
        Err(HierarchyError::FormalCutUnavailable(
            "hierarchy cannot perform two frozen replacements"
        ))
    );
}
