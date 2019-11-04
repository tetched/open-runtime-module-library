#![cfg_attr(not(feature = "std"), no_std)]

use rstd::result;
use sr_primitives::traits::{
	CheckedAdd, CheckedSub, MaybeSerializeDeserialize, Member, SimpleArithmetic, StaticLookup,
};
use srml_support::{decl_event, decl_module, decl_storage, ensure, Parameter};
// FIXME: `srml-` prefix should be used for all srml modules, but currently `srml_system`
// would cause compiling error in `decl_module!` and `construct_runtime!`
// #3295 https://github.com/paritytech/substrate/issues/3295
use srml_system::{self as system, ensure_signed};

use traits::MultiCurrency;

mod imbalances;
pub use imbalances::{NegativeImbalance, PositiveImbalance};

mod mock;
mod tests;

pub trait Trait: srml_system::Trait {
	type Event: From<Event<Self>> + Into<<Self as srml_system::Trait>::Event>;
	type Balance: Parameter + Member + SimpleArithmetic + Default + Copy + MaybeSerializeDeserialize;
	type CurrencyId: Parameter + Member + SimpleArithmetic + Default + Copy + MaybeSerializeDeserialize;
}

decl_storage! {
	trait Store for Module<T: Trait> as Tokens {
		/// The total issuance of a token type;
		pub TotalIssuance get(fn total_issuance) build(|config: &GenesisConfig<T>| {
			let issuance = config.initial_balance * (config.endowed_accounts.len() as u32).into();
			config.tokens.iter().map(|id| (id.clone(), issuance)).collect::<Vec<_>>()
		}): map T::CurrencyId => T::Balance;

		/// The balance of a token type under an account.
		pub Balance get(fn balance): double_map T::CurrencyId, blake2_256(T::AccountId) => T::Balance;
	}
	add_extra_genesis {
		config(tokens): Vec<T::CurrencyId>;
		config(initial_balance): T::Balance;
		config(endowed_accounts): Vec<T::AccountId>;

		build(|config: &GenesisConfig<T>| {
			config.tokens.iter().for_each(|currency_id| {
				config.endowed_accounts.iter().for_each(|account_id| {
					<Balance<T>>::insert(currency_id, account_id, &config.initial_balance);
				})
			})
		})
	}
}

decl_event!(
	pub enum Event<T> where
		<T as srml_system::Trait>::AccountId,
		<T as Trait>::CurrencyId,
		<T as Trait>::Balance
	{
		/// Token transfer success (currency_id, from, to, amount)
		Transferred(CurrencyId, AccountId, AccountId, Balance),
	}
);

decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {
		fn deposit_event() = default;

		/// Transfer some balance to another account.
		pub fn transfer(
			origin,
			dest: <T::Lookup as StaticLookup>::Source,
			#[compact] currency_id: T::CurrencyId,
			#[compact] amount: T::Balance,
		) {
			let from = ensure_signed(origin)?;
			let to = T::Lookup::lookup(dest)?;
			<Self as MultiCurrency<_>>::transfer(currency_id, &from, &to, amount)?;

			Self::deposit_event(RawEvent::Transferred(currency_id, from, to, amount));
		}
	}
}

impl<T: Trait> Module<T> {}

impl<T: Trait> MultiCurrency<T::AccountId> for Module<T> {
	type Balance = T::Balance;
	type CurrencyId = T::CurrencyId;
	type PositiveImbalance = imbalances::PositiveImbalance<T>;
	type NegativeImbalance = imbalances::NegativeImbalance<T>;
	type RebalancePositive = imbalances::RebalancePositive<T>;
	type RebalanceNegative = imbalances::RebalanceNegative<T>;

	fn total_inssuance(currency_id: Self::CurrencyId) -> Self::Balance {
		<TotalIssuance<T>>::get(currency_id)
	}

	fn balance(currency_id: Self::CurrencyId, who: &T::AccountId) -> Self::Balance {
		<Balance<T>>::get(currency_id, who)
	}

	fn transfer(
		currency_id: Self::CurrencyId,
		from: &T::AccountId,
		to: &T::AccountId,
		amount: Self::Balance,
	) -> result::Result<(), &'static str> {
		ensure!(Self::balance(currency_id, from) >= amount, "balance too low",);

		if from != to {
			<Balance<T>>::mutate(currency_id, from, |balance| *balance -= amount);
			<Balance<T>>::mutate(currency_id, to, |balance| *balance += amount);
		}

		Ok(())
	}

	fn deposit(
		currency_id: Self::CurrencyId,
		who: &T::AccountId,
		amount: Self::Balance,
	) -> result::Result<Self::PositiveImbalance, &'static str> {
		ensure!(
			Self::total_issuance(currency_id).checked_add(&amount).is_some(),
			"total issuance overflow after deposit",
		);

		<Balance<T>>::mutate(currency_id, who, |v| *v += amount);

		Ok(<PositiveImbalance<T>>::new(currency_id, amount))
	}

	fn withdraw(
		currency_id: Self::CurrencyId,
		who: &T::AccountId,
		amount: Self::Balance,
	) -> result::Result<Self::NegativeImbalance, &'static str> {
		ensure!(
			Self::balance(currency_id, who).checked_sub(&amount).is_some(),
			"balance too low to withdraw",
		);

		<Balance<T>>::mutate(currency_id, who, |v| *v -= amount);

		Ok(<NegativeImbalance<T>>::new(currency_id, amount))
	}

	fn slash(currency_id: Self::CurrencyId, who: &T::AccountId, amount: Self::Balance) -> Self::NegativeImbalance {
		let actual_amount = Self::balance(currency_id, who).min(amount);
		<Balance<T>>::mutate(currency_id, who, |v| *v -= actual_amount);
		<NegativeImbalance<T>>::new(currency_id, amount)
	}
}