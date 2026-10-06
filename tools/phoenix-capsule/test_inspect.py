import importlib.util
import os
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("capsule_inspect", Path(__file__).with_name("inspect.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class CapsuleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        fixture = os.environ.get("COGNAC_TEST_CAPSULE")
        if not fixture:
            raise unittest.SkipTest("Set COGNAC_TEST_CAPSULE to the verified vendor capsule; it is not distributed with Cognac")
        cls.image = Path(fixture).read_bytes()

    def test_exact_vendor_signature(self):
        result = module.inspect(self.image)
        self.assertTrue(result["embedded_key_signature_matches"])
        self.assertFalse(result["firmware_trust_verified"])

    def test_signed_payload_tamper(self):
        data = bytearray(self.image)
        data[0xc70320] ^= 1
        with self.assertRaisesRegex(ValueError, "digest"):
            module.inspect(data, pin_image=False)

    def test_signature_tamper(self):
        data = bytearray(self.image)
        data[0x220] ^= 1
        with self.assertRaisesRegex(ValueError, "digest"):
            module.inspect(data, pin_image=False)

    def test_excluded_region_still_rejected_by_image_pin(self):
        data = bytearray(self.image)
        data[0x1000] ^= 1
        self.assertTrue(module.inspect(data, pin_image=False)["embedded_key_signature_matches"])
        with self.assertRaisesRegex(ValueError, "differs"):
            module.inspect(data)

    def test_truncated_image(self):
        with self.assertRaisesRegex(ValueError, "length"):
            module.inspect(self.image[:-1])


if __name__ == "__main__":
    unittest.main()
