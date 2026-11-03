import contextlib
import getpass
import importlib.util
import io
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
spec = importlib.util.spec_from_file_location("semantic_review_cli", Path(__file__).resolve().parents[1] / "semantic_review.py")
cli = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cli)


class SemanticCliTests(unittest.TestCase):
    def test_interactive_secret_requires_non_echoing_terminal(self):
        with patch.object(getpass, "getpass", side_effect=getpass.GetPassWarning("no tty")), contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as failure:
                cli.main(["--calibration", "not-read.json", "--run-dir", "not-created", "--ask-key"])
        self.assertEqual(failure.exception.code, 2)


if __name__ == "__main__": unittest.main()
