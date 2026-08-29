"""Unit tests for publish-plan.py: `python3 -m unittest discover -s .github/scripts`."""

import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("publish_plan", HERE / "publish-plan.py")
publish_plan = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish_plan)

precedence = publish_plan.precedence
plan_release = publish_plan.plan_release


def live(*numbers):
    return [(number, False) for number in numbers]


class Precedence(unittest.TestCase):
    def test_the_specification_example_orders_as_written(self):
        # SemVer 2.0.0, section 11.4, verbatim.
        chain = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
        ]
        for lower, higher in zip(chain, chain[1:]):
            self.assertLess(precedence(lower), precedence(higher), (lower, higher))

    def test_numeric_pre_release_fields_compare_as_numbers(self):
        self.assertLess(precedence("0.4.0-rc.9"), precedence("0.4.0-rc.10"))
        self.assertGreater(precedence("0.4.0-rc.10"), precedence("0.4.0-rc.9"))

    def test_build_metadata_does_not_take_part(self):
        self.assertEqual(precedence("1.2.5+20260101"), precedence("1.2.5"))
        self.assertEqual(precedence("1.2.5+a"), precedence("1.2.5+b"))
        self.assertLess(precedence("1.2.5"), precedence("1.2.9+20260101"))

    def test_a_pre_release_ranks_below_its_release(self):
        self.assertLess(precedence("1.0.0-rc.1"), precedence("1.0.0"))

    def test_a_malformed_version_is_an_error_not_a_guess(self):
        with self.assertRaises(ValueError):
            precedence("1.2")
        with self.assertRaises(ValueError):
            precedence("1.x.3")


class Plan(unittest.TestCase):
    def test_rc10_after_rc9_is_allowed(self):
        plan, _, problems = plan_release("0.4.0-rc.10", {"tscn": live("0.3.6", "0.4.0-rc.9")})
        self.assertEqual(problems, [])
        self.assertEqual(plan, {"tscn": True})

    def test_rc9_after_rc10_is_refused(self):
        plan, _, problems = plan_release("0.4.0-rc.9", {"tscn": live("0.3.6", "0.4.0-rc.10")})
        self.assertEqual(
            problems,
            ["tscn 0.4.0-rc.9 is older than 0.4.0-rc.10, which is already on crates.io"],
        )
        self.assertEqual(plan, {"tscn": False})

    def test_build_metadata_on_the_live_version_still_guards(self):
        _, _, problems = plan_release("1.2.5", {"tscn": live("1.2.9+20260101")})
        self.assertEqual(
            problems, ["tscn 1.2.5 is older than 1.2.9+20260101, which is already on crates.io"]
        )

    def test_a_yanked_newer_version_does_not_block_a_corrective_release(self):
        released = {"tscn": [("0.3.6", False), ("0.4.0", True)]}
        plan, notes, problems = plan_release("0.3.7", released)
        self.assertEqual(problems, [])
        self.assertEqual(plan, {"tscn": True})
        self.assertEqual(notes, ["  tscn 0.3.7 needs publishing"])

    def test_a_yanked_same_version_is_named_and_not_republished(self):
        plan, notes, problems = plan_release("0.3.7", {"tscn": [("0.3.7", True)]})
        self.assertEqual(problems, [])
        self.assertEqual(plan, {"tscn": False})
        self.assertEqual(notes, ["  tscn 0.3.7 is on crates.io, yanked"])

    def test_a_half_finished_release_completes(self):
        released = {"tscn": live("0.3.5", "0.3.6"), "gdmerge": live("0.3.5")}
        plan, _, problems = plan_release("0.3.6", released)
        self.assertEqual(problems, [])
        self.assertEqual(plan, {"tscn": False, "gdmerge": True})

    def test_a_new_crate_is_published(self):
        plan, _, problems = plan_release("0.1.0", {"tscn": []})
        self.assertEqual(problems, [])
        self.assertEqual(plan, {"tscn": True})


if __name__ == "__main__":
    unittest.main()
