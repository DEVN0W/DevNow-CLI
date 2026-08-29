#[derive(Clone, Copy)]
pub(crate) struct LogoCell {
    pub(crate) top: (u8, u8, u8),
    pub(crate) bottom: (u8, u8, u8),
}

pub(crate) const DEVNOW_LOGO_WIDE: [[LogoCell; 12]; 8] = [
    [
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (69, 29, 20),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (221, 81, 45),
        },
        LogoCell {
            top: (60, 27, 18),
            bottom: (242, 89, 46),
        },
        LogoCell {
            top: (60, 27, 18),
            bottom: (241, 89, 45),
        },
        LogoCell {
            top: (48, 23, 16),
            bottom: (236, 87, 44),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (199, 75, 39),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (81, 35, 21),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
    ],
    [
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (205, 74, 44),
            bottom: (221, 78, 47),
        },
        LogoCell {
            top: (255, 92, 53),
            bottom: (242, 86, 50),
        },
        LogoCell {
            top: (239, 86, 49),
            bottom: (235, 85, 49),
        },
        LogoCell {
            top: (241, 88, 46),
            bottom: (238, 86, 49),
        },
        LogoCell {
            top: (242, 89, 45),
            bottom: (237, 87, 46),
        },
        LogoCell {
            top: (251, 92, 46),
            bottom: (233, 86, 44),
        },
        LogoCell {
            top: (255, 96, 48),
            bottom: (241, 88, 44),
        },
        LogoCell {
            top: (98, 41, 24),
            bottom: (228, 85, 43),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
    ],
    [
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (133, 48, 32),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (255, 92, 56),
            bottom: (79, 32, 23),
        },
        LogoCell {
            top: (253, 89, 52),
            bottom: (144, 53, 34),
        },
        LogoCell {
            top: (255, 91, 53),
            bottom: (144, 53, 33),
        },
        LogoCell {
            top: (255, 93, 53),
            bottom: (130, 50, 31),
        },
        LogoCell {
            top: (255, 94, 51),
            bottom: (125, 48, 30),
        },
        LogoCell {
            top: (254, 93, 49),
            bottom: (139, 53, 31),
        },
        LogoCell {
            top: (255, 99, 49),
            bottom: (148, 57, 32),
        },
        LogoCell {
            top: (125, 49, 27),
            bottom: (85, 36, 22),
        },
    ],
    [
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (127, 42, 33),
        },
        LogoCell {
            top: (68, 26, 21),
            bottom: (252, 79, 57),
        },
        LogoCell {
            top: (152, 52, 36),
            bottom: (255, 82, 57),
        },
        LogoCell {
            top: (190, 64, 43),
            bottom: (253, 81, 55),
        },
        LogoCell {
            top: (195, 67, 44),
            bottom: (251, 82, 55),
        },
        LogoCell {
            top: (166, 60, 38),
            bottom: (255, 86, 55),
        },
        LogoCell {
            top: (83, 33, 22),
            bottom: (255, 91, 54),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (182, 66, 39),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
    ],
    [
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (96, 31, 27),
            bottom: (207, 53, 49),
        },
        LogoCell {
            top: (255, 73, 59),
            bottom: (246, 64, 57),
        },
        LogoCell {
            top: (237, 71, 54),
            bottom: (237, 66, 54),
        },
        LogoCell {
            top: (236, 73, 54),
            bottom: (239, 70, 54),
        },
        LogoCell {
            top: (238, 75, 54),
            bottom: (239, 74, 55),
        },
        LogoCell {
            top: (240, 77, 53),
            bottom: (239, 75, 55),
        },
        LogoCell {
            top: (239, 78, 52),
            bottom: (240, 77, 54),
        },
        LogoCell {
            top: (234, 78, 52),
            bottom: (242, 78, 52),
        },
        LogoCell {
            top: (255, 90, 56),
            bottom: (239, 79, 52),
        },
        LogoCell {
            top: (158, 58, 36),
            bottom: (244, 83, 53),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (62, 26, 19),
        },
    ],
    [
        LogoCell {
            top: (59, 19, 20),
            bottom: (72, 20, 23),
        },
        LogoCell {
            top: (229, 53, 54),
            bottom: (221, 46, 52),
        },
        LogoCell {
            top: (233, 57, 55),
            bottom: (223, 51, 52),
        },
        LogoCell {
            top: (238, 62, 55),
            bottom: (232, 55, 54),
        },
        LogoCell {
            top: (239, 65, 55),
            bottom: (237, 60, 56),
        },
        LogoCell {
            top: (239, 69, 54),
            bottom: (239, 64, 55),
        },
        LogoCell {
            top: (239, 72, 54),
            bottom: (239, 67, 54),
        },
        LogoCell {
            top: (239, 74, 54),
            bottom: (239, 71, 54),
        },
        LogoCell {
            top: (240, 76, 53),
            bottom: (239, 74, 54),
        },
        LogoCell {
            top: (239, 77, 52),
            bottom: (236, 75, 53),
        },
        LogoCell {
            top: (253, 83, 55),
            bottom: (253, 81, 56),
        },
        LogoCell {
            top: (129, 46, 32),
            bottom: (150, 52, 36),
        },
    ],
    [
        LogoCell {
            top: (48, 15, 18),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (203, 38, 49),
            bottom: (163, 30, 41),
        },
        LogoCell {
            top: (212, 43, 50),
            bottom: (210, 38, 51),
        },
        LogoCell {
            top: (221, 49, 52),
            bottom: (206, 41, 49),
        },
        LogoCell {
            top: (230, 54, 53),
            bottom: (219, 47, 51),
        },
        LogoCell {
            top: (236, 58, 55),
            bottom: (227, 52, 52),
        },
        LogoCell {
            top: (239, 63, 55),
            bottom: (234, 57, 54),
        },
        LogoCell {
            top: (239, 66, 54),
            bottom: (238, 61, 55),
        },
        LogoCell {
            top: (239, 69, 55),
            bottom: (238, 64, 55),
        },
        LogoCell {
            top: (236, 72, 54),
            bottom: (240, 68, 54),
        },
        LogoCell {
            top: (251, 79, 57),
            bottom: (239, 72, 55),
        },
        LogoCell {
            top: (122, 43, 31),
            bottom: (49, 21, 18),
        },
    ],
    [
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (61, 16, 20),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (202, 34, 50),
            bottom: (84, 19, 25),
        },
        LogoCell {
            top: (199, 36, 49),
            bottom: (194, 33, 48),
        },
        LogoCell {
            top: (205, 39, 49),
            bottom: (196, 34, 48),
        },
        LogoCell {
            top: (215, 44, 51),
            bottom: (202, 38, 49),
        },
        LogoCell {
            top: (224, 50, 52),
            bottom: (212, 43, 51),
        },
        LogoCell {
            top: (231, 55, 54),
            bottom: (219, 48, 51),
        },
        LogoCell {
            top: (235, 59, 55),
            bottom: (236, 55, 55),
        },
        LogoCell {
            top: (255, 68, 59),
            bottom: (154, 41, 39),
        },
        LogoCell {
            top: (131, 40, 34),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
    ],
];

pub(crate) const DEVNOW_LOGO_MEDIUM: [[LogoCell; 8]; 5] = [
    [
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (40, 20, 15),
            bottom: (202, 73, 43),
        },
        LogoCell {
            top: (128, 50, 29),
            bottom: (255, 94, 52),
        },
        LogoCell {
            top: (132, 52, 28),
            bottom: (249, 91, 47),
        },
        LogoCell {
            top: (93, 39, 23),
            bottom: (253, 93, 46),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (177, 67, 35),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
    ],
    [
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (204, 72, 44),
            bottom: (36, 18, 15),
        },
        LogoCell {
            top: (253, 90, 52),
            bottom: (160, 57, 37),
        },
        LogoCell {
            top: (242, 87, 50),
            bottom: (167, 61, 38),
        },
        LogoCell {
            top: (251, 91, 49),
            bottom: (126, 48, 30),
        },
        LogoCell {
            top: (255, 103, 51),
            bottom: (111, 44, 26),
        },
        LogoCell {
            top: (149, 57, 31),
            bottom: (96, 39, 23),
        },
    ],
    [
        LogoCell {
            top: (0, 0, 0),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (59, 23, 19),
            bottom: (225, 62, 52),
        },
        LogoCell {
            top: (197, 63, 46),
            bottom: (255, 75, 58),
        },
        LogoCell {
            top: (227, 74, 50),
            bottom: (241, 76, 55),
        },
        LogoCell {
            top: (229, 77, 50),
            bottom: (243, 78, 54),
        },
        LogoCell {
            top: (202, 71, 44),
            bottom: (254, 83, 55),
        },
        LogoCell {
            top: (68, 29, 20),
            bottom: (252, 86, 53),
        },
        LogoCell {
            top: (0, 0, 0),
            bottom: (71, 30, 20),
        },
    ],
    [
        LogoCell {
            top: (116, 30, 31),
            bottom: (117, 26, 32),
        },
        LogoCell {
            top: (246, 59, 57),
            bottom: (226, 47, 53),
        },
        LogoCell {
            top: (233, 62, 54),
            bottom: (227, 54, 53),
        },
        LogoCell {
            top: (239, 68, 54),
            bottom: (238, 60, 55),
        },
        LogoCell {
            top: (239, 73, 54),
            bottom: (240, 66, 55),
        },
        LogoCell {
            top: (235, 75, 53),
            bottom: (236, 70, 54),
        },
        LogoCell {
            top: (252, 82, 55),
            bottom: (248, 78, 56),
        },
        LogoCell {
            top: (170, 59, 39),
            bottom: (182, 60, 42),
        },
    ],
    [
        LogoCell {
            top: (50, 15, 18),
            bottom: (0, 0, 0),
        },
        LogoCell {
            top: (200, 36, 49),
            bottom: (94, 20, 27),
        },
        LogoCell {
            top: (211, 42, 50),
            bottom: (201, 35, 49),
        },
        LogoCell {
            top: (223, 49, 52),
            bottom: (203, 38, 49),
        },
        LogoCell {
            top: (234, 57, 54),
            bottom: (216, 46, 51),
        },
        LogoCell {
            top: (238, 63, 55),
            bottom: (240, 57, 56),
        },
        LogoCell {
            top: (253, 72, 57),
            bottom: (151, 42, 38),
        },
        LogoCell {
            top: (101, 35, 28),
            bottom: (0, 0, 0),
        },
    ],
];
