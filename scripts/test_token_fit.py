import os
import unittest

import token_fit as tf


class ContentClass(unittest.TestCase):
    def test_classes(self):
        self.assertEqual(tf.content_class('{"items": [1, 2], "total": 2}'), "json")
        self.assertEqual(tf.content_class('{"a": 1}\n{"a": 2}\n{"a": 3}\n'), "json")
        self.assertEqual(tf.content_class("fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n"), "code")
        self.assertEqual(tf.content_class("ok 1\nok 2\ntest result: ok. 2 passed\n"), "prose")
        self.assertEqual(tf.content_class("ファイルが見つかりません: 設定.json\n"), "cjk")
        # Starts like JSON but isn't: judged by its lines.
        self.assertEqual(tf.content_class("[not json at all\nplain words here\nmore words\n"), "prose")


class Fit(unittest.TestCase):
    def test_a_three_bytes_per_token_tokenizer(self):
        texts = [("a", "word " * 300), ("b", "other words " * 100), ("c", "x = 1;\n" * 200)]
        counts = [len(t.encode()) // 3 + 5 for _, t in texts]   # 5 tokens of message framing
        result = tf.fit(texts, counts, overhead=5)
        prose = result["prose"]
        self.assertAlmostEqual(prose["bytes_per_token"], 3.0, places=2)
        self.assertAlmostEqual(prose["error_fitted"]["median"], 0.0, places=2)
        # bytes ÷ 4 undercounts a 3-bytes-per-token text by a quarter.
        self.assertAlmostEqual(prose["error_bytes_div_4"]["total"], -0.25, places=2)
        self.assertEqual(result["code"]["texts"], 1)

    def test_no_key_is_a_clear_error(self):
        env = os.environ.pop("ANTHROPIC_API_KEY", None)
        try:
            self.assertEqual(tf.main([]), 2)
        finally:
            if env is not None:
                os.environ["ANTHROPIC_API_KEY"] = env


if __name__ == "__main__":
    unittest.main()
