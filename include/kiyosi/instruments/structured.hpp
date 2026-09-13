#pragma once

#include <cmath>
#include <vector>
#include <utility>
#include <kiyosi/core/types.hpp>
#include <kiyosi/core/schedule.hpp>

namespace kiyosi {
enum class observation_frequency { daily,
                                   at_expiry };
enum class barrier_touch_status { none,
                                  up,
                                  down };

class AutocallableNote {
public:
    double initial_price() const noexcept { return terms_.initial_price; }
    const std::vector<double>& knock_out_prices() const noexcept { return terms_.knock_out_prices; }
    double upper_strike() const noexcept { return terms_.upper_strike; }
    double lower_strike() const noexcept { return terms_.lower_strike; }
    const std::vector<date>& observation_dates() const noexcept { return terms_.observation_dates; }
    double principal_ratio() const noexcept { return terms_.principal_ratio; }
    date effective() const noexcept { return terms_.effective; }
    date expiry() const noexcept { return terms_.expiry; }
    barrier_touch_status touch_status() const noexcept { return terms_.touch_status; }
    friend bool operator==(const AutocallableNote&, const AutocallableNote&) = default;

private:
    AutocallableNote(double initial_price, std::vector<double> knock_out_prices, double upper_strike,
                     double lower_strike, std::vector<date> observations, double principal_ratio,
                     barrier_touch_status touch_status, date effective, date expiry)
        : terms_{initial_price, std::move(knock_out_prices), upper_strike, lower_strike,
                 std::move(observations), principal_ratio, touch_status, effective, expiry} {}

private:
    struct Terms {
        double initial_price;
        std::vector<double> knock_out_prices;
        double upper_strike;
        double lower_strike;
        std::vector<date> observation_dates;
        double principal_ratio;
        barrier_touch_status touch_status;
        date effective;
        date expiry;
        friend bool operator==(const Terms&, const Terms&) = default;
    } terms_;

    friend class KiAutocallableNote;
    friend class BinarySnowballOption;
};

class KiAutocallableNote {
public:
    double initial_price() const noexcept { return note_.initial_price(); }
    const std::vector<double>& knock_out_prices() const noexcept { return note_.knock_out_prices(); }
    double upper_strike() const noexcept { return note_.upper_strike(); }
    double lower_strike() const noexcept { return note_.lower_strike(); }
    const std::vector<date>& observation_dates() const noexcept { return note_.observation_dates(); }
    double principal_ratio() const noexcept { return note_.principal_ratio(); }
    date effective() const noexcept { return note_.effective(); }
    date expiry() const noexcept { return note_.expiry(); }
    barrier_touch_status touch_status() const noexcept { return note_.touch_status(); }
    double knock_in_price() const noexcept { return knock_in_price_; }
    observation_frequency knock_in_frequency() const noexcept { return frequency_; }
    friend bool operator==(const KiAutocallableNote&, const KiAutocallableNote&) = default;

private:
    KiAutocallableNote(double initial_price, double knock_in_price, std::vector<double> knock_out_prices,
                       double upper_strike, double lower_strike, std::vector<date> observations,
                       observation_frequency frequency, barrier_touch_status touch_status, double principal_ratio,
                       date effective, date expiry)
        : note_(initial_price, std::move(knock_out_prices), upper_strike, lower_strike,
                std::move(observations), principal_ratio, touch_status, effective, expiry),
          knock_in_price_(knock_in_price), frequency_(frequency) {}

private:
    AutocallableNote note_;
    double knock_in_price_;
    observation_frequency frequency_;

    friend class PhoenixOption;
    friend class SnowballOption;
    friend class TernarySnowballOption;
};

class PhoenixOption {
public:
    double initial_price() const noexcept { return note_.initial_price(); }
    const std::vector<double>& knock_out_prices() const noexcept { return note_.knock_out_prices(); }
    double upper_strike() const noexcept { return note_.upper_strike(); }
    double lower_strike() const noexcept { return note_.lower_strike(); }
    const std::vector<date>& observation_dates() const noexcept { return note_.observation_dates(); }
    double principal_ratio() const noexcept { return note_.principal_ratio(); }
    date effective() const noexcept { return note_.effective(); }
    date expiry() const noexcept { return note_.expiry(); }
    barrier_touch_status touch_status() const noexcept { return note_.touch_status(); }
    double knock_in_price() const noexcept { return note_.knock_in_price(); }
    observation_frequency knock_in_frequency() const noexcept { return note_.knock_in_frequency(); }
    result<PhoenixOption> with_coupon_rate(double coupon) const;
    double coupon_rate() const noexcept { return coupon_rate_; }
    const std::vector<double>& coupon_barriers() const noexcept { return coupon_barriers_; }

private:
    PhoenixOption(double coupon_rate, double initial_price, double knock_in_price, std::vector<double> knock_out_prices,
                  std::vector<double> coupon_barriers, double upper_strike, double lower_strike,
                  std::vector<date> observations, observation_frequency frequency, barrier_touch_status touch_status,
                  double principal_ratio, date effective, date expiry)
        : note_(initial_price, knock_in_price, std::move(knock_out_prices), upper_strike, lower_strike,
                std::move(observations), frequency, touch_status, principal_ratio, effective, expiry),
          coupon_rate_(coupon_rate), coupon_barriers_(std::move(coupon_barriers)) {}
    friend result<PhoenixOption> make_phoenix_option(double, double, double, std::vector<double>, std::vector<double>, double, double,
                                                     std::vector<date>, observation_frequency, barrier_touch_status, double, date, date);
    KiAutocallableNote note_;
    double coupon_rate_;
    std::vector<double> coupon_barriers_;
};

class SnowballOption {
public:
    double initial_price() const noexcept { return note_.initial_price(); }
    const std::vector<double>& knock_out_prices() const noexcept { return note_.knock_out_prices(); }
    double upper_strike() const noexcept { return note_.upper_strike(); }
    double lower_strike() const noexcept { return note_.lower_strike(); }
    const std::vector<date>& observation_dates() const noexcept { return note_.observation_dates(); }
    double principal_ratio() const noexcept { return note_.principal_ratio(); }
    date effective() const noexcept { return note_.effective(); }
    date expiry() const noexcept { return note_.expiry(); }
    barrier_touch_status touch_status() const noexcept { return note_.touch_status(); }
    double knock_in_price() const noexcept { return note_.knock_in_price(); }
    observation_frequency knock_in_frequency() const noexcept { return note_.knock_in_frequency(); }
    result<SnowballOption> with_coupon_rate(double coupon) const;
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }

private:
    SnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                   double initial_price, double knock_in_price, std::vector<double> knock_out_prices,
                   double upper_strike, double lower_strike, std::vector<date> observations,
                   observation_frequency frequency, barrier_touch_status touch_status, double principal_ratio,
                   date effective, date expiry)
        : note_(initial_price, knock_in_price, std::move(knock_out_prices), upper_strike, lower_strike,
                std::move(observations), frequency, touch_status, principal_ratio, effective, expiry),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)), maturity_coupon_rate_(maturity_coupon_rate) {}
    friend result<SnowballOption> make_snowball_option(std::vector<double>, double, double, double, std::vector<double>, double, double,
                                                       std::vector<date>, observation_frequency, barrier_touch_status, double, date, date);
    KiAutocallableNote note_;
    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;
};

class BinarySnowballOption {
public:
    double initial_price() const noexcept { return note_.initial_price(); }
    const std::vector<double>& knock_out_prices() const noexcept { return note_.knock_out_prices(); }
    double upper_strike() const noexcept { return note_.upper_strike(); }
    double lower_strike() const noexcept { return note_.lower_strike(); }
    const std::vector<date>& observation_dates() const noexcept { return note_.observation_dates(); }
    double principal_ratio() const noexcept { return note_.principal_ratio(); }
    date effective() const noexcept { return note_.effective(); }
    date expiry() const noexcept { return note_.expiry(); }
    barrier_touch_status touch_status() const noexcept { return note_.touch_status(); }
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }

private:
    BinarySnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                         double initial_price, std::vector<double> knock_out_prices, double upper_strike,
                         double lower_strike, std::vector<date> observations, barrier_touch_status touch_status,
                         double principal_ratio, date effective, date expiry)
        : note_(initial_price, std::move(knock_out_prices), upper_strike, lower_strike,
                std::move(observations), principal_ratio, touch_status, effective, expiry),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)), maturity_coupon_rate_(maturity_coupon_rate) {}
    friend result<BinarySnowballOption> make_binary_snowball_option(std::vector<double>, double, double, std::vector<double>, double, double,
                                                                    std::vector<date>, barrier_touch_status, double, date, date);
    AutocallableNote note_;
    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;
};

class TernarySnowballOption {
public:
    double initial_price() const noexcept { return note_.initial_price(); }
    const std::vector<double>& knock_out_prices() const noexcept { return note_.knock_out_prices(); }
    double upper_strike() const noexcept { return note_.upper_strike(); }
    double lower_strike() const noexcept { return note_.lower_strike(); }
    const std::vector<date>& observation_dates() const noexcept { return note_.observation_dates(); }
    double principal_ratio() const noexcept { return note_.principal_ratio(); }
    date effective() const noexcept { return note_.effective(); }
    date expiry() const noexcept { return note_.expiry(); }
    barrier_touch_status touch_status() const noexcept { return note_.touch_status(); }
    double knock_in_price() const noexcept { return note_.knock_in_price(); }
    observation_frequency knock_in_frequency() const noexcept { return note_.knock_in_frequency(); }
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }
    double minimal_coupon_rate() const noexcept { return minimal_coupon_rate_; }

private:
    TernarySnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                          double minimal_coupon_rate, double initial_price, double knock_in_price,
                          std::vector<double> knock_out_prices, double upper_strike, double lower_strike,
                          std::vector<date> observations, observation_frequency frequency,
                          barrier_touch_status touch_status, double principal_ratio, date effective, date expiry)
        : note_(initial_price, knock_in_price, std::move(knock_out_prices), upper_strike, lower_strike,
                std::move(observations), frequency, touch_status, principal_ratio, effective, expiry),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)), maturity_coupon_rate_(maturity_coupon_rate),
          minimal_coupon_rate_(minimal_coupon_rate) {}
    friend result<TernarySnowballOption> make_ternary_snowball_option(std::vector<double>, double, double, double, double, std::vector<double>,
                                                                      double, double, std::vector<date>, observation_frequency,
                                                                      barrier_touch_status, double, date, date);
    KiAutocallableNote note_;
    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_, minimal_coupon_rate_;
};

template <typename Note>
[[nodiscard]] inline result<Note> validate_note(Note note)
{
    if (!std::isfinite(note.initial_price()) || note.initial_price() <= 0.0 ||
        !std::isfinite(note.upper_strike()) || note.upper_strike() <= 0.0 ||
        !std::isfinite(note.lower_strike()) || note.lower_strike() < 0.0 || note.lower_strike() > note.upper_strike() ||
        !std::isfinite(note.principal_ratio()) || note.principal_ratio() < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter, "autocallable terms are invalid"});
    if (note.observation_dates().empty() || note.knock_out_prices().size() != note.observation_dates().size())
        return std::unexpected(Error{error_category::invalid_schedule, "autocallable schedule is invalid"});
    auto schedule = validate_date_schedule(note.observation_dates(), note.effective(), note.expiry());
    if (!schedule)
        return std::unexpected(Error{error_category::invalid_schedule, "autocallable schedule is invalid"});
    for (std::size_t index = 0; index < note.observation_dates().size(); ++index) {
        if (!std::isfinite(note.knock_out_prices()[index]) || note.knock_out_prices()[index] <= 0.0)
            return std::unexpected(Error{error_category::invalid_parameter, "knock-out prices are invalid"});
    }
    if (note.touch_status() != barrier_touch_status::none && note.touch_status() != barrier_touch_status::up &&
        note.touch_status() != barrier_touch_status::down)
        return std::unexpected(Error{error_category::invalid_parameter, "autocallable touch status is invalid"});
    if constexpr (requires { note.knock_in_price(); }) {
        if (!std::isfinite(note.knock_in_price()) || note.knock_in_price() <= 0.0 ||
            (note.knock_in_frequency() != observation_frequency::daily &&
             note.knock_in_frequency() != observation_frequency::at_expiry))
            return std::unexpected(Error{error_category::invalid_parameter, "knock-in terms are invalid"});
    }
    if constexpr (requires { note.coupon_rate(); }) {
        if (!std::isfinite(note.coupon_rate()))
            return std::unexpected(Error{error_category::invalid_parameter, "Phoenix coupon is invalid"});
        if (note.coupon_barriers().size() != note.observation_dates().size())
            return std::unexpected(Error{error_category::invalid_schedule, "Phoenix coupon schedule is invalid"});
        for (double barrier : note.coupon_barriers())
            if (!std::isfinite(barrier) || barrier < 0.0)
                return std::unexpected(Error{error_category::invalid_parameter, "coupon barriers are invalid"});
    }
    if constexpr (requires { note.knock_out_coupon_rates(); }) {
        if (note.knock_out_coupon_rates().size() != note.observation_dates().size())
            return std::unexpected(Error{error_category::invalid_schedule, "coupon schedule count is invalid"});
        for (double coupon : note.knock_out_coupon_rates())
            if (!std::isfinite(coupon))
                return std::unexpected(Error{error_category::invalid_parameter, "coupon rates are invalid"});
        if (!std::isfinite(note.maturity_coupon_rate()))
            return std::unexpected(Error{error_category::invalid_parameter, "maturity coupon is invalid"});
    }
    if constexpr (requires { note.minimal_coupon_rate(); }) {
        if (!std::isfinite(note.minimal_coupon_rate()))
            return std::unexpected(Error{error_category::invalid_parameter, "minimal coupon is invalid"});
    }
    return note;
}

[[nodiscard]] inline result<PhoenixOption> make_phoenix_option(
    double coupon_rate, double initial_price, double knock_in_price, std::vector<double> knock_out_prices,
    std::vector<double> coupon_barriers, double upper_strike, double lower_strike, std::vector<date> observations,
    observation_frequency frequency, barrier_touch_status touch_status, double principal_ratio, date effective, date expiry)
{ return validate_note(PhoenixOption{coupon_rate, initial_price, knock_in_price, std::move(knock_out_prices),
                                     std::move(coupon_barriers), upper_strike, lower_strike, std::move(observations),
                                     frequency, touch_status, principal_ratio, effective, expiry}); }

[[nodiscard]] inline result<SnowballOption> make_snowball_option(
    std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate, double initial_price,
    double knock_in_price, std::vector<double> knock_out_prices, double upper_strike, double lower_strike,
    std::vector<date> observations, observation_frequency frequency, barrier_touch_status touch_status,
    double principal_ratio, date effective, date expiry)
{ return validate_note(SnowballOption{std::move(knock_out_coupon_rates), maturity_coupon_rate, initial_price,
                                      knock_in_price, std::move(knock_out_prices), upper_strike, lower_strike,
                                      std::move(observations), frequency, touch_status, principal_ratio, effective, expiry}); }

[[nodiscard]] inline result<SnowballOption> make_standard_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    return make_snowball_option(
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price, knock_in_price,
        std::vector<double>(observations.size(), knock_out_price), initial_price, 0.0, observations,
        observation_frequency::daily, touch_status, principal_ratio, effective, expiry);
}

[[nodiscard]] inline result<SnowballOption> make_step_down_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_start,
    double knock_out_step, std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    std::vector<double> knock_out_prices;
    knock_out_prices.reserve(observations.size());
    for (std::size_t index = 0; index < observations.size(); ++index)
        knock_out_prices.push_back(knock_out_start - index * knock_out_step);
    return make_snowball_option(
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price, knock_in_price,
        std::move(knock_out_prices), initial_price, 0.0, observations, observation_frequency::daily,
        touch_status, principal_ratio, effective, expiry);
}

[[nodiscard]] inline result<SnowballOption> make_both_down_snowball(
    double coupon_start, double coupon_step, double initial_price, double knock_in_price,
    double knock_out_start, double knock_out_step, std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    std::vector<double> coupons;
    std::vector<double> knock_out_prices;
    coupons.reserve(observations.size());
    knock_out_prices.reserve(observations.size());
    for (std::size_t index = 0; index < observations.size(); ++index) {
        coupons.push_back(coupon_start - index * coupon_step);
        knock_out_prices.push_back(knock_out_start - index * knock_out_step);
    }
    const double maturity_coupon = coupons.empty() ? 0.0 : coupons.back();
    return make_snowball_option(
        std::move(coupons), maturity_coupon, initial_price, knock_in_price, std::move(knock_out_prices),
        initial_price, 0.0, observations, observation_frequency::daily, touch_status,
        principal_ratio, effective, expiry);
}

[[nodiscard]] inline result<SnowballOption> make_dual_coupon_snowball(
    double knock_out_coupon, double maturity_coupon, double initial_price, double knock_in_price,
    double knock_out_price, std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    return make_snowball_option(
        std::vector<double>(observations.size(), knock_out_coupon), maturity_coupon, initial_price, knock_in_price,
        std::vector<double>(observations.size(), knock_out_price), initial_price, 0.0, observations,
        observation_frequency::daily, touch_status, principal_ratio, effective, expiry);
}

[[nodiscard]] inline result<SnowballOption> make_parachute_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    double final_knock_out_price, std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    std::vector<double> knock_out_prices(observations.size(), knock_out_price);
    if (!knock_out_prices.empty()) knock_out_prices.back() = final_knock_out_price;
    return make_snowball_option(
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price, knock_in_price,
        std::move(knock_out_prices), initial_price, 0.0, observations, observation_frequency::daily,
        touch_status, principal_ratio, effective, expiry);
}

[[nodiscard]] inline result<SnowballOption> make_otm_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    double upper_strike, std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    return make_snowball_option(
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price, knock_in_price,
        std::vector<double>(observations.size(), knock_out_price), upper_strike, 0.0, observations,
        observation_frequency::daily, touch_status, principal_ratio, effective, expiry);
}

[[nodiscard]] inline result<SnowballOption> make_loss_capped_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    double lower_strike, std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    return make_snowball_option(
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price, knock_in_price,
        std::vector<double>(observations.size(), knock_out_price), initial_price, lower_strike,
        observations, observation_frequency::daily, touch_status, principal_ratio, effective, expiry);
}

[[nodiscard]] inline result<SnowballOption> make_european_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    return make_snowball_option(
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price, knock_in_price,
        std::vector<double>(observations.size(), knock_out_price), initial_price, 0.0, observations,
        observation_frequency::at_expiry, touch_status, principal_ratio, effective, expiry);
}

[[nodiscard]] inline result<BinarySnowballOption> make_binary_snowball_option(
    std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate, double initial_price,
    std::vector<double> knock_out_prices, double upper_strike, double lower_strike, std::vector<date> observations,
    barrier_touch_status touch_status, double principal_ratio, date effective, date expiry)
{ return validate_note(BinarySnowballOption{std::move(knock_out_coupon_rates), maturity_coupon_rate, initial_price,
                                             std::move(knock_out_prices), upper_strike, lower_strike,
                                             std::move(observations), touch_status, principal_ratio, effective, expiry}); }

[[nodiscard]] inline result<TernarySnowballOption> make_ternary_snowball_option(
    std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate, double minimal_coupon_rate,
    double initial_price, double knock_in_price, std::vector<double> knock_out_prices, double upper_strike,
    double lower_strike, std::vector<date> observations, observation_frequency frequency,
    barrier_touch_status touch_status, double principal_ratio, date effective, date expiry)
{ return validate_note(TernarySnowballOption{std::move(knock_out_coupon_rates), maturity_coupon_rate, minimal_coupon_rate,
                                              initial_price, knock_in_price, std::move(knock_out_prices), upper_strike,
                                              lower_strike, std::move(observations), frequency, touch_status,
                                              principal_ratio, effective, expiry}); }

inline result<PhoenixOption> PhoenixOption::with_coupon_rate(double coupon) const
{
    return make_phoenix_option(coupon, initial_price(), knock_in_price(), knock_out_prices(), coupon_barriers(),
                               upper_strike(), lower_strike(), observation_dates(), knock_in_frequency(),
                               touch_status(), principal_ratio(), effective(), expiry());
}

inline result<SnowballOption> SnowballOption::with_coupon_rate(double coupon) const
{
    return make_snowball_option(knock_out_coupon_rates(), coupon, initial_price(), knock_in_price(),
                                knock_out_prices(), upper_strike(), lower_strike(), observation_dates(),
                                knock_in_frequency(), touch_status(), principal_ratio(), effective(), expiry());
}
} // namespace kiyosi
