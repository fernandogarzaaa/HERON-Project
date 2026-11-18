import unittest


class MathTests(unittest.TestCase):
    def test_addition(self):
        self.assertEqual(2 + 2, 4)

    def test_multiplication(self):
        self.assertEqual(3 * 4, 12)

    def test_subtraction(self):
        self.assertEqual(10 - 4, 6)


if __name__ == "__main__":
    unittest.main()
