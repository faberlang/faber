import unittest

import compare


class CompareSectionsTest(unittest.TestCase):
    def setUp(self):
        self.expected = {
            "hashes": b"hash-0\n",
            "summary": b"tick,value\n0,10\n",
            "snapshot_0": b"snapshot-zero\n",
            "snapshot_100": b"snapshot-hundred\n",
            "snapshot_500": b"snapshot-five-hundred\n",
        }

    def output(self, **overrides):
        sections = dict(self.expected)
        sections.update(overrides)
        return b"startup output\n" + b"".join(
            b"== " + name.encode("ascii") + b"\n" + body
            for name, body in sections.items()
        )

    def test_valid_full_result_matches_all_sections(self):
        results = compare.compare_sections(self.expected, self.output())
        self.assertEqual(results, {name: "match" for name in self.expected})

    def test_different_newline_bytes_are_a_difference(self):
        results = compare.compare_sections(self.expected, self.output(hashes=b"hash-0\r\n"))
        self.assertEqual(results["hashes"], {
            "line": 0,
            "want": "hash-0\\n",
            "got": "hash-0\\r\\n",
        })

    def test_missing_section_is_reported(self):
        sections = dict(self.expected)
        del sections["snapshot_100"]
        output = b"".join(
            b"== " + name.encode("ascii") + b"\n" + body
            for name, body in sections.items()
        )
        results = compare.compare_sections(self.expected, output)
        self.assertEqual(results["snapshot_100"], "missing")

    def test_changed_bytes_are_reported(self):
        results = compare.compare_sections(self.expected, self.output(snapshot_0=b"Snapshot-zero\n"))
        self.assertEqual(results["snapshot_0"], {
            "line": 0,
            "want": "snapshot-zero\\n",
            "got": "Snapshot-zero\\n",
        })


if __name__ == "__main__":
    unittest.main()
