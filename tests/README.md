# Tests

Install the Python test extra from the repository root, then run the default suite:

```sh
python -m pip install '.[test]'
python -m unittest discover -s tests/python -p 'test_*.py'
```

The extra pins QuantLib 1.43. It is required: missing QuantLib fails discovery rather than skipping price validation. QuantLib is not needed to use Kiyosi or to build and test the C++ core.

`test_quantlib_reference.py` compares live QuantLib prices and Greeks with the public Kiyosi Python APIs, including scalar price differences and implied-volatility recovery. `test_structured_reference.py` checks terminal-payoff portfolios and finite-difference convergence. Reference construction and explicit scenarios live in `tests/python/quantlib_reference`; there is no reference-generation command or committed pricing snapshot to refresh.

The references use Actual/365 Fixed, continuously compounded BSM parameters, `WeekendsOnly` matched to Kiyosi's `weekdays_calendar`, and unrolled contract dates. Reference uncertainty checks are independent of Kiyosi engine error budgets. American references refine 800, 1600 and 3200 point grids and check bump stability, so full test runs take longer than snapshot comparisons. Arithmetic Asian checks compare the same Levy approximation, not the true arithmetic-average value. Structured checks use basic-payoff portfolios, not native structured-product engines.

The QuantLib calculations change a global evaluation date. Tests restore settings with `SavedSettings`; run comparisons serially within a process, or use separate processes for parallel runs. `api_parity.tsv` remains a shared C++/Python API contract fixture.
