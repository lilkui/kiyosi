#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <new>
#include <utility>

#include <kiyosi/kiyosi.hpp>

namespace {
bool& fail_next_allocation()
{
    static thread_local bool fail = false;
    return fail;
}

template <typename Value>
bool check_assignment(const char* name, const Value& original, const Value& replacement)
{
    auto value = original;
    bool allocation_failed = false;
    fail_next_allocation() = true;
    try {
        value = replacement;
    } catch (const std::bad_alloc&) {
        allocation_failed = true;
    }
    fail_next_allocation() = false;
    if (!allocation_failed || value != original) {
        std::cerr << name << ": failed assignment changed the original value\n";
        return false;
    }
    value = replacement;
    if (value != replacement) {
        std::cerr << name << ": successful assignment did not copy every field\n";
        return false;
    }
    return true;
}
} // namespace

// A separate executable keeps allocation failure injection out of the other tests and their runners.
// NOLINTBEGIN(cppcoreguidelines-no-malloc,readability-inconsistent-declaration-parameter-name): replace global allocation without recursion or platform-specific parameter names.
void* operator new(std::size_t size)
{
    if (std::exchange(fail_next_allocation(), false)) throw std::bad_alloc{};
    if (void* result = std::malloc(size == 0 ? 1 : size)) return result;
    throw std::bad_alloc{};
}

void operator delete(void* pointer) noexcept { std::free(pointer); }
void operator delete(void* pointer, std::size_t) noexcept { std::free(pointer); }
// NOLINTEND(cppcoreguidelines-no-malloc,readability-inconsistent-declaration-parameter-name)

int main()
try {
    using namespace kiyosi;
    const Date start{std::chrono::year{2025} / 1 / 1}, end{std::chrono::year{2026} / 1 / 1};
    const auto original = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 90.0, .barrier_type = BarrierType::down_and_in});
    const auto replacement = *make_barrier_option({.option_type = OptionType::put, .strike = 110.0, .effective_date = start + std::chrono::days{1}, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out, .rebate = 5.0, .rebate_timing = RebateTiming::at_hit, .observation_mode = ObservationMode::scheduled, .observation_dates = {end}});
    const auto binary = *make_asset_binary_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 90.0, .barrier_type = BarrierType::down_and_in});
    const auto binary_replacement = *make_cash_binary_barrier_option({.option_type = OptionType::put, .strike = 110.0, .effective_date = start + std::chrono::days{1}, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = {end}}, 7.0);
    const auto touch = *make_asset_one_touch_down(start, end, 90.0);
    const auto touch_replacement = *make_cash_one_touch_up(start + std::chrono::days{1}, end, 120.0, 7.0,
                                                           SettlementTiming::at_hit, ObservationMode::scheduled, {end});
    const bool terms_ok = check_assignment("BarrierTerms", original.barrier_terms(), replacement.barrier_terms());
    const bool option_ok = check_assignment("BarrierOption", original, replacement);
    const bool binary_ok = check_assignment("BinaryBarrierOption", binary, binary_replacement);
    const bool touch_ok = check_assignment("TouchOption", touch, touch_replacement);
    const auto double_original = *make_double_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .lower_barrier = 80.0, .upper_barrier = 120.0, .barrier_type = DoubleBarrierType::knock_in});
    const auto double_replacement = *make_double_barrier_option({.option_type = OptionType::put, .strike = 110.0, .effective_date = start + std::chrono::days{1}, .expiry_date = end, .lower_barrier = 90.0, .upper_barrier = 130.0, .barrier_type = DoubleBarrierType::knock_out, .rebate = 5.0, .rebate_timing = RebateTiming::at_hit, .observation_mode = ObservationMode::scheduled, .observation_dates = {end}, .touch_state = BarrierTouchState::untouched});
    const bool double_terms_ok = check_assignment("DoubleBarrierTerms", double_original.barrier_terms(), double_replacement.barrier_terms());
    const bool double_option_ok = check_assignment("DoubleBarrierOption", double_original, double_replacement);
    return terms_ok && option_ok && binary_ok && touch_ok && double_terms_ok && double_option_ok ? 0 : 1;
} catch (const std::exception& error) {
    std::fputs(error.what(), stderr);
    std::fputs("\n", stderr);
    return 1;
}
