// Authored W5 tonal bands and view envelope, in the reviewed cell layout.
const START: u32 = 156;
const CENTER: f64 = 73.0;
const LIMITS: Limits = Limits {
    dark_floor: 77,
    bearing_start: 96,
    highlight_limit: 144,
};
const BANDS: &[Band<'_>] = &[
    Band {
        row: 14,
        stops: &[(60, 0), (65, -4), (69, -6), (73, -6), (78, -1), (79, 0)],
    },
    Band {
        row: 15,
        stops: &[
            (58, 0),
            (62, -5),
            (66, -13),
            (72, -19),
            (78, -18),
            (84, -10),
            (89, -3),
            (91, 0),
        ],
    },
    Band {
        row: 16,
        stops: &[
            (61, 0),
            (65, -5),
            (70, -13),
            (76, -24),
            (82, -19),
            (88, -10),
            (92, -3),
            (94, 0),
        ],
    },
    Band {
        row: 17,
        stops: &[
            (66, 0),
            (71, -5),
            (76, -11),
            (82, -15),
            (89, -9),
            (91, -4),
            (94, 0),
        ],
    },
    Band {
        row: 18,
        stops: &[
            (65, 0),
            (69, 2),
            (75, 3),
            (80, 4),
            (85, 4),
            (90, 1),
            (92, 0),
        ],
    },
    Band {
        row: 19,
        stops: &[
            (63, 0),
            (68, 3),
            (74, 7),
            (79, 11),
            (84, 11),
            (89, 8),
            (93, 2),
            (94, 0),
        ],
    },
    Band {
        row: 20,
        stops: &[
            (66, 0),
            (71, 3),
            (76, 8),
            (81, 12),
            (85, 10),
            (89, 6),
            (93, 1),
            (94, 0),
        ],
    },
    Band {
        row: 21,
        stops: &[
            (72, 0),
            (75, 2),
            (79, 6),
            (82, 7),
            (86, 4),
            (90, 1),
            (92, 0),
        ],
    },
];
const VIEWS: &[View] = &[
    View {
        gain: 0.00,
        horizontal_scale: 1.06,
        center: 73.50,
    },
    View {
        gain: 0.10,
        horizontal_scale: 1.06,
        center: 73.50,
    },
    View {
        gain: 0.22,
        horizontal_scale: 1.05,
        center: 73.50,
    },
    View {
        gain: 0.36,
        horizontal_scale: 1.05,
        center: 73.40,
    },
    View {
        gain: 0.50,
        horizontal_scale: 1.04,
        center: 73.40,
    },
    View {
        gain: 0.62,
        horizontal_scale: 1.04,
        center: 73.30,
    },
    View {
        gain: 0.74,
        horizontal_scale: 1.03,
        center: 73.30,
    },
    View {
        gain: 0.85,
        horizontal_scale: 1.02,
        center: 73.20,
    },
    View {
        gain: 0.94,
        horizontal_scale: 1.02,
        center: 73.20,
    },
    View {
        gain: 1.00,
        horizontal_scale: 1.01,
        center: 73.10,
    },
    View {
        gain: 1.00,
        horizontal_scale: 1.01,
        center: 73.10,
    },
    View {
        gain: 1.00,
        horizontal_scale: 1.00,
        center: 73.00,
    },
    View {
        gain: 1.00,
        horizontal_scale: 1.00,
        center: 73.00,
    },
    View {
        gain: 1.00,
        horizontal_scale: 1.00,
        center: 72.90,
    },
    View {
        gain: 1.00,
        horizontal_scale: 0.99,
        center: 72.90,
    },
    View {
        gain: 1.00,
        horizontal_scale: 0.98,
        center: 72.80,
    },
    View {
        gain: 0.94,
        horizontal_scale: 0.97,
        center: 72.80,
    },
    View {
        gain: 0.84,
        horizontal_scale: 0.96,
        center: 72.70,
    },
    View {
        gain: 0.73,
        horizontal_scale: 0.95,
        center: 72.60,
    },
    View {
        gain: 0.61,
        horizontal_scale: 0.94,
        center: 72.60,
    },
    View {
        gain: 0.50,
        horizontal_scale: 0.93,
        center: 72.50,
    },
    View {
        gain: 0.37,
        horizontal_scale: 0.92,
        center: 72.40,
    },
    View {
        gain: 0.24,
        horizontal_scale: 0.92,
        center: 72.30,
    },
    View {
        gain: 0.11,
        horizontal_scale: 0.91,
        center: 72.30,
    },
    View {
        gain: 0.00,
        horizontal_scale: 0.91,
        center: 72.20,
    },
];
