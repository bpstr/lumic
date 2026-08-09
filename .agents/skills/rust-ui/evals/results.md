# Rust/UI skill evaluation

Two representative prompts were run with and without the skill, then graded
against `evals.json`.

| Evaluation | With skill | Baseline | Material difference |
| --- | ---: | ---: | --- |
| Grouped sidenav shell | 6/6 | 5/6 | The skill selected the complete Rust/UI Sidenav composition; baseline used generic navigation markup. |
| Node settings form | 5/5 | 2/5 | The skill selected Field, Select, Checkbox, Spinner, and their state semantics; baseline largely hand-rolled native markup. |
| Total | 11/11 | 7/11 | Rust/UI-specific composition and primitive selection improved. |

The first sidenav run renamed explicitly requested groups. The skill was updated
to preserve user-specified labels and rerun successfully. It was also tightened
to prohibit copied demo identity, node, environment, or health data and to
require an accessible mobile Sheet title.

Remaining verification boundary: representative snippets must still be checked
against the current copied registry source and compiled in the target project,
because the public component API can change.
