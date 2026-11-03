import importlib.machinery
import importlib.util
import unittest
from pathlib import Path


loader = importlib.machinery.SourceFileLoader("perf_baseline", str(Path(__file__).parents[1] / "perf-baseline"))
spec = importlib.util.spec_from_loader(loader.name, loader)
module = importlib.util.module_from_spec(spec)
loader.exec_module(module)


class PerformanceBaselineTests(unittest.TestCase):
    def test_macos_ps_cpu_time_is_not_read_as_centisecond_seconds(self):
        self.assertEqual(module.parse_ps_cpu_time("0:00.52"), 0.52)
        self.assertEqual(module.parse_ps_cpu_time("0:05.40"), 5.4)
        self.assertEqual(module.parse_ps_cpu_time("1:02:03.04"), 3723.04)
        self.assertEqual(module.parse_ps_cpu_time("2-00:00:01"), 172801.0)

    def test_idle_summary_reports_observed_range(self):
        summary = module.summary([
            {"rss_kb": 100, "cpu_percent": 1.0},
            {"rss_kb": 120, "cpu_percent": 3.0},
        ])
        self.assertEqual(summary, {
            "sample_count": 2,
            "rss_kb_min": 100,
            "rss_kb_max": 120,
            "rss_kb_avg": 110,
            "cpu_percent_min": 1.0,
            "cpu_percent_max": 3.0,
            "cpu_percent_avg": 2.0,
        })

    def test_launcher_timeout_contains_startup_observation_and_stop(self):
        self.assertEqual(module.qa_timeout_for(10), 45)
        self.assertEqual(module.qa_timeout_for(45), 80)
        self.assertEqual(module.qa_timeout_for(120), 155)
