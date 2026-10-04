"""Focused tests for source rewrites performed after benchmark simulation."""

import random
import unittest
from decimal import Decimal

from gen import Person
from nativeize import _files, _native_accounts, _native_journal_lines, _native_line


class NativeOpeningTests(unittest.TestCase):
    def test_seed_accounts_become_one_law_free_opening_group(self):
        paths = {
            "p1/bank/checking": "p1-checking",
            "p1/bank/savings": "p1-savings",
            "p1/retire/k401": "p1-401k",
            "p1/edu/plan529": "p1-529",
        }
        source = [
            "2024-01-01 p1/opening -> p1/bank/checking 30000.00 USD",
            "2024-01-01 p1/opening -> p1/bank/savings 20000.00 USD",
            "2024-01-01 p1/opening -> p1/retire/k401 60000.00 USD",
            "2024-01-01 p1/opening -> p1/edu/plan529 10000.00 USD",
            "2024-01-01 p1/bank/checking -> p1/bank/savings 400.00 USD",
        ]

        output, opening_rows = _native_journal_lines(
            source, paths, {}, {}, {}, {"p1/opening"}
        )

        self.assertEqual(opening_rows, 4)
        self.assertEqual(len(output), len(source) + 1, "four holding rows remain with one header")
        self.assertEqual(
            output,
            [
                "opening 2024-01-01",
                "  p1-checking 30000.00 USD",
                "  p1-savings 20000.00 USD",
                "  p1-401k 60000.00 USD",
                "  p1-529 10000.00 USD",
                "2024-01-01 p1-checking -> p1-savings 400.00 USD #contribution",
            ],
        )
        self.assertFalse(any("p1-opening" in line for line in output))

    def test_education_category_keeps_tuition_qualification_and_combined_limit(self):
        person = Person(0, random.Random(7), "full", 12)
        places, parties, leaves, payees = _files([person])
        declarations = _native_accounts([person], True, 1, 1.0)
        self.assertIn("purpose p1-edu : spending", declarations)
        self.assertNotIn("entity p1-opening", declarations)
        self.assertIn("purpose p1-edu-tuition : education", declarations)
        self.assertIn("purpose p1-edu-books : p1-edu", declarations)
        combined_cap = (
            "value(total(#p1-edu, month), USD) + "
            "value(total(#p1-edu-tuition, month), USD) <= 12_000 USD"
        )
        self.assertIn("law p1-edu-budget-nonqualified", declarations)
        self.assertIn("law p1-edu-budget-tuition", declarations)
        self.assertEqual(declarations.count("when owner is p1"), 2)
        self.assertEqual(declarations.count(combined_cap), 2)
        self.assertNotIn("purpose spending", declarations)

        accounts = set(places.values())
        tuition = _native_line(
            "2024-08-15 p1/edu/plan529 -> p1/edu/tuition/university 1200.00 USD",
            places, parties, leaves, payees, accounts,
        )
        other_education = _native_line(
            "2024-08-15 p1/bank/checking -> p1/edu/books/textbooks 35.00 USD",
            places, parties, leaves, payees, accounts,
        )
        self.assertIn("#p1-edu-tuition-university", tuition)
        self.assertIn("#p1-edu-books-textbooks", other_education)
        amounts = [line.split()[4] for line in (tuition, other_education)]
        self.assertEqual(amounts, ["1200.00", "35.00"])
        self.assertEqual(sum(map(Decimal, amounts)), Decimal("1235.00"))


class V5SpellingTests(unittest.TestCase):
    places = {
        "p1/bank/checking": "p1-checking",
        "p1/bank/savings": "p1-savings",
        "p1/invest/brokerage": "p1-brokerage",
        "p1/travel/eur-wallet": "p1-eur-wallet",
        "p1/retire/k401": "p1-401k",
    }
    parties = {"income/p1/interest": "p1-interest-source", "acme-p1": "acme-p1"}

    def line(self, text):
        return _native_line(text, self.places, self.parties, {}, {}, set(self.places.values()))

    def test_a_payment_that_arrives_is_written_from_the_books_own_side(self):
        written = self.line("2024-01-28 income/p1/interest -> p1/bank/savings 5.10 USD")
        self.assertTrue(written.startswith("2024-01-28 p1-savings <- p1-interest-source 5.10 USD"), written)

    def test_a_paystub_passes_the_money_through_its_owner_and_each_leg_leads_with_its_arrow(self):
        self.assertEqual(self.line("2024-01-15 acme-p1 -> 7681.50 USD"), "2024-01-15 p1 <- acme-p1 7681.50 USD")
        self.assertEqual(self.line("  p1/retire/k401 800.00 USD"), "  -> p1-401k 800.00 USD")

    def test_an_exchange_states_one_amount_and_its_price(self):
        sale = self.line("2024-02-05 p1/invest/brokerage 3 VTI -> p1/bank/checking 660.00 USD")
        self.assertTrue(sale.startswith("2024-02-05 p1-brokerage 3 VTI -> p1-checking @ 220.00 USD"), sale)
        trip = self.line("2024-06-03 p1/bank/checking 1060.00 USD -> p1/travel/eur-wallet 1000.00 EUR")
        self.assertTrue(trip.startswith("2024-06-03 p1-checking -> p1-eur-wallet 1000.00 EUR @ 1.06 USD"), trip)


if __name__ == "__main__":
    unittest.main()
