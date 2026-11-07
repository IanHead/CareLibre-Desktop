//! Field and step definitions for the intake wizards.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Multiline,
    Email,
    Phone,
    Date,
    Select,
    Checkbox,
}

pub struct FieldSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub placeholder: &'static str,
    pub kind: FieldKind,
    pub required: bool,
    pub options: &'static [&'static str],
    pub wide: bool,
}

pub struct StepSpec {
    pub title: &'static str,
    pub description: &'static str,
    pub fields: &'static [FieldSpec],
}

const fn base(
    key: &'static str,
    label: &'static str,
    placeholder: &'static str,
    kind: FieldKind,
    required: bool,
    wide: bool,
) -> FieldSpec {
    FieldSpec {
        key,
        label,
        placeholder,
        kind,
        required,
        options: &[],
        wide,
    }
}

pub const fn txt(key: &'static str, label: &'static str, placeholder: &'static str) -> FieldSpec {
    base(key, label, placeholder, FieldKind::Text, false, false)
}

pub const fn txt_req(
    key: &'static str,
    label: &'static str,
    placeholder: &'static str,
) -> FieldSpec {
    base(key, label, placeholder, FieldKind::Text, true, false)
}

pub const fn wide(key: &'static str, label: &'static str, placeholder: &'static str) -> FieldSpec {
    base(key, label, placeholder, FieldKind::Multiline, false, true)
}

pub const fn wide_req(
    key: &'static str,
    label: &'static str,
    placeholder: &'static str,
) -> FieldSpec {
    base(key, label, placeholder, FieldKind::Multiline, true, true)
}

pub const fn mail(key: &'static str, label: &'static str, placeholder: &'static str) -> FieldSpec {
    base(key, label, placeholder, FieldKind::Email, false, false)
}

pub const fn mail_req(
    key: &'static str,
    label: &'static str,
    placeholder: &'static str,
) -> FieldSpec {
    base(key, label, placeholder, FieldKind::Email, true, false)
}

pub const fn phone_req(
    key: &'static str,
    label: &'static str,
    placeholder: &'static str,
) -> FieldSpec {
    base(key, label, placeholder, FieldKind::Phone, true, false)
}

pub const fn date(key: &'static str, label: &'static str, placeholder: &'static str) -> FieldSpec {
    base(key, label, placeholder, FieldKind::Date, false, false)
}

pub const fn date_req(
    key: &'static str,
    label: &'static str,
    placeholder: &'static str,
) -> FieldSpec {
    base(key, label, placeholder, FieldKind::Date, true, false)
}

pub const fn sel(
    key: &'static str,
    label: &'static str,
    options: &'static [&'static str],
) -> FieldSpec {
    FieldSpec {
        key,
        label,
        placeholder: "Select…",
        kind: FieldKind::Select,
        required: false,
        options,
        wide: false,
    }
}

pub const fn sel_req(
    key: &'static str,
    label: &'static str,
    options: &'static [&'static str],
) -> FieldSpec {
    FieldSpec {
        key,
        label,
        placeholder: "Select…",
        kind: FieldKind::Select,
        required: true,
        options,
        wide: false,
    }
}

pub const fn check(key: &'static str, label: &'static str) -> FieldSpec {
    FieldSpec {
        key,
        label,
        placeholder: "",
        kind: FieldKind::Checkbox,
        required: false,
        options: &[],
        wide: true,
    }
}

pub const fn check_req(key: &'static str, label: &'static str) -> FieldSpec {
    FieldSpec {
        key,
        label,
        placeholder: "",
        kind: FieldKind::Checkbox,
        required: true,
        options: &[],
        wide: true,
    }
}

const STATES: &[&str] = &["NSW", "VIC", "QLD", "WA", "SA", "TAS", "ACT", "NT"];
const DIVISIONS: &[&str] = &[
    "Community Care",
    "Aged Care",
    "Disability Services",
    "Respite",
    "Allied Health",
];

pub const CLIENT_STEPS: &[StepSpec] = &[
    StepSpec {
        title: "Personal details",
        description: "The participant's identity and background.",
        fields: &[
            txt_req("first_name", "First name", "e.g. Margaret"),
            txt_req("last_name", "Last name", "e.g. Chen"),
            txt("preferred_name", "Preferred name", "Optional"),
            date("date_of_birth", "Date of birth", "DD/MM/YYYY"),
            sel(
                "gender",
                "Gender",
                &["Female", "Male", "Non-binary", "Prefer not to say"],
            ),
            txt("pronouns", "Pronouns", "e.g. she/her"),
            txt("languages", "Languages spoken", "e.g. English, Cantonese"),
            txt("cultural_identity", "Cultural identity", "Optional"),
        ],
    },
    StepSpec {
        title: "Contact & address",
        description: "How to reach the participant and where they live.",
        fields: &[
            mail("email", "Email", "name@example.com"),
            phone_req("phone", "Phone", "0400 000 000"),
            txt_req("address_line1", "Address line 1", "Street address"),
            txt("address_line2", "Address line 2", "Unit, building, level"),
            txt_req("suburb", "Suburb", "Suburb"),
            sel_req("state", "State", STATES),
            txt_req("postcode", "Postcode", "2000"),
            txt(
                "preferred_contact",
                "Preferred contact method",
                "e.g. phone, SMS or email",
            ),
        ],
    },
    StepSpec {
        title: "Health & support needs",
        description: "Clinical background and the support the participant needs.",
        fields: &[
            wide(
                "primary_disability",
                "Primary disability or diagnosis",
                "e.g. autism, acquired brain injury, age-related frailty",
            ),
            wide(
                "health_conditions",
                "Health conditions",
                "Medical conditions and diagnoses",
            ),
            wide(
                "medications",
                "Medications",
                "Current medications and dosage",
            ),
            wide(
                "allergies",
                "Allergies",
                "Food, medication or environmental allergies",
            ),
            txt(
                "mobility",
                "Mobility needs",
                "Aids, transfers, access requirements",
            ),
            txt(
                "communication",
                "Communication needs",
                "AAC, interpreter, hearing or vision support",
            ),
            wide("health_notes", "Other health notes", "Optional"),
        ],
    },
    StepSpec {
        title: "Support network",
        description: "Emergency and professional contacts around the participant.",
        fields: &[
            txt_req("emergency_contact_name", "Emergency contact", "Full name"),
            phone_req("emergency_contact_phone", "Emergency phone", "0400 000 000"),
            txt(
                "emergency_contact_relationship",
                "Relationship",
                "e.g. daughter, friend",
            ),
            txt("gp_name", "GP / doctor", "Full name"),
            txt("gp_phone", "GP phone", "Phone number"),
            txt(
                "support_coordinator",
                "Support coordinator",
                "Name and organisation",
            ),
            txt("plan_manager", "Plan manager", "Name and organisation"),
            txt("advocate", "Advocate or nominee", "Name and relationship"),
        ],
    },
    StepSpec {
        title: "Behaviours & risk",
        description: "What the care team needs to know to keep everyone safe.",
        fields: &[
            wide(
                "behaviours",
                "Behaviours of concern",
                "What do they look like? Frequency and intensity",
            ),
            wide(
                "triggers",
                "Triggers and warning signs",
                "Known triggers or early warning signs",
            ),
            wide(
                "strategies",
                "Calming strategies",
                "What works well for this participant",
            ),
            wide(
                "risk_notes",
                "Risk notes",
                "Environmental, medical or safety risks",
            ),
            check("behaviour_plan", "Behaviour support plan on file"),
            check("restrictive_practices", "Restrictive practices in place"),
        ],
    },
    StepSpec {
        title: "Funding & consent",
        description: "Funding source and consent to hold and share information.",
        fields: &[
            sel_req(
                "funding_type",
                "Funding type",
                &[
                    "NDIS",
                    "Aged Care (CHSP)",
                    "Aged Care (SAH)",
                    "Private",
                    "Other",
                ],
            ),
            txt("plan_number", "Plan or reference number", "Optional"),
            date("plan_start", "Plan start", "DD/MM/YYYY"),
            date("plan_end", "Plan end", "DD/MM/YYYY"),
            wide(
                "intake_notes",
                "Intake notes",
                "Anything else the care team should know",
            ),
            check_req(
                "consent_share",
                "Consent to share information with the care team",
            ),
            check_req(
                "consent_plan",
                "Consent to develop and maintain a care plan",
            ),
        ],
    },
];

pub const WORKER_STEPS: &[StepSpec] = &[
    StepSpec {
        title: "Personal & contact details",
        description: "Identity and contact information for the worker.",
        fields: &[
            txt_req("first_name", "First name", "e.g. Sam"),
            txt_req("last_name", "Last name", "e.g. Taylor"),
            txt("preferred_name", "Preferred name", "Optional"),
            date("date_of_birth", "Date of birth", "DD/MM/YYYY"),
            sel(
                "gender",
                "Gender",
                &["Female", "Male", "Non-binary", "Prefer not to say"],
            ),
            mail_req("email", "Email", "name@example.com"),
            phone_req("phone", "Phone", "0400 000 000"),
            txt(
                "preferred_contact",
                "Preferred contact method",
                "e.g. phone, SMS or email",
            ),
            txt_req("address_line1", "Address line 1", "Street address"),
            txt("address_line2", "Address line 2", "Unit, building, level"),
            txt_req("suburb", "Suburb", "Suburb"),
            sel_req("state", "State", STATES),
            txt_req("postcode", "Postcode", "2000"),
        ],
    },
    StepSpec {
        title: "Position & employment",
        description: "Role, division and the terms of employment.",
        fields: &[
            txt_req(
                "position",
                "Position title",
                "e.g. Disability Support Worker",
            ),
            sel_req("division", "Division", DIVISIONS),
            sel_req(
                "employment_type",
                "Employment type",
                &["Casual", "Part-time", "Full-time", "Contract"],
            ),
            date_req("start_date", "Contract start", "DD/MM/YYYY"),
            date("end_date", "Contract end", "DD/MM/YYYY"),
            txt("ordinary_hours", "Ordinary hours per week", "e.g. 25"),
            txt("reports_to", "Reports to", "Manager or supervisor"),
            wide("employment_notes", "Employment notes", "Optional"),
        ],
    },
    StepSpec {
        title: "Award & classification",
        description: "Industrial instrument and pay classification.",
        fields: &[
            wide_req(
                "award",
                "Award",
                "e.g. Social, Community, Home Care and Disability Services Industry Award",
            ),
            txt_req(
                "classification",
                "Classification",
                "e.g. Level 2, Pay point 3",
            ),
            txt("base_rate", "Base hourly rate", "$"),
            txt("superannuation", "Superannuation", "%"),
            wide("award_notes", "Award notes", "Optional"),
        ],
    },
    StepSpec {
        title: "Compliance & checks",
        description: "Screening, checks and expiry dates.",
        fields: &[
            txt("ndis_screening", "NDIS worker screening number", "Number"),
            date("ndis_expiry", "NDIS screening expiry", "DD/MM/YYYY"),
            txt("police_check", "Police check reference", "Reference"),
            date("police_check_date", "Police check date", "DD/MM/YYYY"),
            txt("wwcc", "Working with Children number", "Number"),
            date("wwcc_expiry", "WWCC expiry", "DD/MM/YYYY"),
            date("first_aid_expiry", "First aid expiry", "DD/MM/YYYY"),
            txt("drivers_licence", "Driver's licence", "Number and class"),
            check("vehicle_access", "Has access to a vehicle"),
            wide("compliance_notes", "Compliance notes", "Optional"),
        ],
    },
    StepSpec {
        title: "Review & confirm",
        description: "Final notes and confirmation that the information is accurate.",
        fields: &[
            wide(
                "worker_notes",
                "Notes",
                "Anything else the team should know",
            ),
            check_req("accurate", "I confirm the information provided is accurate"),
            check_req(
                "consent_share",
                "Consent to store this information and share it with the care team",
            ),
        ],
    },
];

pub const OTHER_STEPS: &[StepSpec] = &[
    StepSpec {
        title: "Contact details",
        description: "Who this contact is and how to reach them.",
        fields: &[
            txt_req("first_name", "First name", "e.g. Priya"),
            txt_req("last_name", "Last name", "e.g. Nair"),
            txt("organisation", "Organisation", "Optional"),
            sel_req(
                "contact_type",
                "Contact type",
                &[
                    "Family / nominee",
                    "Support coordinator",
                    "GP / health professional",
                    "Plan manager",
                    "Other",
                ],
            ),
            txt(
                "role_relationship",
                "Role or relationship",
                "e.g. daughter, support coordinator",
            ),
            mail("email", "Email", "name@example.com"),
            phone_req("phone", "Phone", "0400 000 000"),
            txt(
                "preferred_contact",
                "Preferred contact method",
                "e.g. phone, SMS or email",
            ),
        ],
    },
    StepSpec {
        title: "Address",
        description: "Postal or business address (optional).",
        fields: &[
            txt("address_line1", "Address line 1", "Street address"),
            txt("address_line2", "Address line 2", "Unit, building, level"),
            txt("suburb", "Suburb", "Suburb"),
            sel("state", "State", STATES),
            txt("postcode", "Postcode", "2000"),
        ],
    },
    StepSpec {
        title: "Notes & consent",
        description: "Anything else and consent to store the contact.",
        fields: &[
            wide("notes", "Notes", "Optional"),
            check_req(
                "consent_share",
                "Consent to store this contact's information",
            ),
        ],
    },
];

pub fn steps(kind: crate::intake::IntakeKind) -> &'static [StepSpec] {
    match kind {
        crate::intake::IntakeKind::Client => CLIENT_STEPS,
        crate::intake::IntakeKind::Worker => WORKER_STEPS,
        crate::intake::IntakeKind::Other => OTHER_STEPS,
    }
}
