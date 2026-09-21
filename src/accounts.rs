//! A typed account layer.
//!
//! Accounts are deliberately not one enum with a flag.  A material account
//! is a contract with a party, a virtual account is a query and recognizer
//! view, and a book account is a book-scoped projection of a material
//! account.  Their different Rust types make crossing those boundaries an
//! explicit operation instead of an accidental string conversion.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

macro_rules! id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn try_new(value: impl Into<String>) -> Result<Self, AccountError> {
                let value = value.into();
                if value.is_empty() {
                    return Err(AccountError::EmptyIdentity {
                        kind: stringify!($name),
                    });
                }
                if value.chars().any(char::is_control) {
                    return Err(AccountError::InvalidIdentity {
                        kind: stringify!($name),
                        value,
                    });
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

id!(MaterialAccountId);
id!(VirtualViewId);
id!(BookAccountId);
id!(PartyId);
id!(BookId);

/// Roles are closed, so a declaration cannot smuggle an unvalidated role
/// into the contract boundary as an arbitrary string.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum Role {
    Owner,
    Holder,
    Issuer,
    Custodian,
    Counterparty,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum Instrument {
    Cash,
    Security,
    Commodity,
    Liability,
    Service,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountError {
    EmptyIdentity {
        kind: &'static str,
    },
    InvalidIdentity {
        kind: &'static str,
        value: String,
    },
    EmptyRoles,
    EmptyInstruments,
    InvalidRoleForInstrument {
        role: Role,
        instrument: Instrument,
    },
    DuplicateMaterial(MaterialAccountId),
    DuplicateVirtual(VirtualViewId),
    DuplicateBookAccount {
        book: BookId,
        account: BookAccountId,
    },
    UnknownMaterial(MaterialAccountId),
    UnknownBook(BookId),
    InvalidBookScope {
        account: BookAccountId,
        book: BookId,
    },
    EmptyQuery,
    EmptyQuerySet,
    QueryInstrumentNotAccepted {
        account: MaterialAccountId,
        instrument: Instrument,
    },
    QueryRoleNotDeclared {
        account: MaterialAccountId,
        role: Role,
    },
    DuplicateMapping {
        instrument: Instrument,
        role: Role,
    },
    MappingSourceUnknown(MaterialAccountId),
    MappingInstrumentNotAccepted {
        account: MaterialAccountId,
        instrument: Instrument,
    },
    MappingRoleNotDeclared {
        account: MaterialAccountId,
        role: Role,
    },
}

impl fmt::Display for AccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for AccountError {}

/// The contract boundary shared by material accounts and their book-scoped
/// projections.  The sets are sorted to make validation and serialization
/// deterministic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountContract {
    party: PartyId,
    roles: BTreeSet<Role>,
    instruments: BTreeSet<Instrument>,
}

impl AccountContract {
    pub fn try_new(
        party: PartyId,
        roles: impl IntoIterator<Item = Role>,
        instruments: impl IntoIterator<Item = Instrument>,
    ) -> Result<Self, AccountError> {
        let roles = roles.into_iter().collect::<BTreeSet<_>>();
        let instruments = instruments.into_iter().collect::<BTreeSet<_>>();
        let contract = Self {
            party,
            roles,
            instruments,
        };
        contract.validate()?;
        Ok(contract)
    }

    fn validate(&self) -> Result<(), AccountError> {
        if self.roles.is_empty() {
            return Err(AccountError::EmptyRoles);
        }
        if self.instruments.is_empty() {
            return Err(AccountError::EmptyInstruments);
        }
        for &role in &self.roles {
            for &instrument in &self.instruments {
                if !role_accepts(role, instrument) {
                    return Err(AccountError::InvalidRoleForInstrument { role, instrument });
                }
            }
        }
        Ok(())
    }

    pub fn party(&self) -> &PartyId {
        &self.party
    }

    pub fn roles(&self) -> &BTreeSet<Role> {
        &self.roles
    }

    pub fn instruments(&self) -> &BTreeSet<Instrument> {
        &self.instruments
    }

    pub fn accepts(&self, instrument: Instrument, role: Role) -> bool {
        self.instruments.contains(&instrument)
            && self.roles.contains(&role)
            && role_accepts(role, instrument)
    }
}

fn role_accepts(role: Role, instrument: Instrument) -> bool {
    match role {
        Role::Owner | Role::Holder => true,
        Role::Issuer => matches!(instrument, Instrument::Security | Instrument::Liability),
        Role::Custodian => matches!(
            instrument,
            Instrument::Cash | Instrument::Security | Instrument::Commodity
        ),
        Role::Counterparty => matches!(
            instrument,
            Instrument::Cash | Instrument::Liability | Instrument::Service
        ),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterialAccount {
    pub id: MaterialAccountId,
    pub contract: AccountContract,
}

impl MaterialAccount {
    pub fn try_new(id: MaterialAccountId, contract: AccountContract) -> Result<Self, AccountError> {
        contract.validate()?;
        Ok(Self { id, contract })
    }
}

/// A small, closed query language.  It is intentionally not a stringly
/// expression: references remain nominal material-account identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ViewExpression {
    Position {
        account: MaterialAccountId,
        instrument: Instrument,
    },
    ByRole {
        account: MaterialAccountId,
        role: Role,
    },
    InBook(BookId),
    Union(Vec<ViewExpression>),
    Intersection(Vec<ViewExpression>),
}

impl ViewExpression {
    pub fn position(account: MaterialAccountId, instrument: Instrument) -> Self {
        Self::Position {
            account,
            instrument,
        }
    }

    pub fn validate(
        &self,
        materials: &BTreeMap<MaterialAccountId, MaterialAccount>,
        books: &BTreeSet<BookId>,
    ) -> Result<(), AccountError> {
        match self {
            Self::Position {
                account,
                instrument,
            } => {
                let material = materials
                    .get(account)
                    .ok_or_else(|| AccountError::UnknownMaterial(account.clone()))?;
                if !material.contract.instruments.contains(instrument) {
                    return Err(AccountError::QueryInstrumentNotAccepted {
                        account: account.clone(),
                        instrument: *instrument,
                    });
                }
            }
            Self::ByRole { account, role } => {
                let material = materials
                    .get(account)
                    .ok_or_else(|| AccountError::UnknownMaterial(account.clone()))?;
                if !material.contract.roles.contains(role) {
                    return Err(AccountError::QueryRoleNotDeclared {
                        account: account.clone(),
                        role: *role,
                    });
                }
            }
            Self::InBook(book) => {
                if !books.contains(book) {
                    return Err(AccountError::UnknownBook(book.clone()));
                }
            }
            Self::Union(terms) | Self::Intersection(terms) => {
                if terms.is_empty() {
                    return Err(AccountError::EmptyQuerySet);
                }
                for term in terms {
                    term.validate(materials, books)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum ViewField {
    Quantity,
    Balance,
    EventCount,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecognitionRule {
    pub instrument: Instrument,
    pub role: Role,
    pub output: ViewField,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecognizerMapping {
    pub source: MaterialAccountId,
    pub rules: Vec<RecognitionRule>,
}

impl RecognizerMapping {
    pub fn try_new(
        source: MaterialAccountId,
        rules: impl IntoIterator<Item = RecognitionRule>,
        materials: &BTreeMap<MaterialAccountId, MaterialAccount>,
    ) -> Result<Self, AccountError> {
        let material = materials
            .get(&source)
            .ok_or_else(|| AccountError::MappingSourceUnknown(source.clone()))?;
        let mut seen = BTreeSet::new();
        let rules = rules.into_iter().collect::<Vec<_>>();
        for rule in &rules {
            if !material.contract.instruments.contains(&rule.instrument) {
                return Err(AccountError::MappingInstrumentNotAccepted {
                    account: source.clone(),
                    instrument: rule.instrument,
                });
            }
            if !material.contract.roles.contains(&rule.role) {
                return Err(AccountError::MappingRoleNotDeclared {
                    account: source.clone(),
                    role: rule.role,
                });
            }
            if !seen.insert((rule.instrument, rule.role)) {
                return Err(AccountError::DuplicateMapping {
                    instrument: rule.instrument,
                    role: rule.role,
                });
            }
        }
        if rules.is_empty() {
            return Err(AccountError::EmptyQuery);
        }
        Ok(Self { source, rules })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualView {
    pub id: VirtualViewId,
    pub query: ViewExpression,
    pub recognizer: RecognizerMapping,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookAccount {
    pub id: BookAccountId,
    pub book: BookId,
    pub material: MaterialAccountId,
    pub instruments: BTreeSet<Instrument>,
}

impl BookAccount {
    fn try_new(
        id: BookAccountId,
        book: BookId,
        material: &MaterialAccount,
        instruments: impl IntoIterator<Item = Instrument>,
    ) -> Result<Self, AccountError> {
        let instruments = instruments.into_iter().collect::<BTreeSet<_>>();
        if instruments.is_empty() {
            return Err(AccountError::EmptyInstruments);
        }
        if instruments
            .iter()
            .any(|instrument| !material.contract.instruments.contains(instrument))
        {
            let instrument = *instruments
                .iter()
                .find(|instrument| !material.contract.instruments.contains(instrument))
                .expect("any() found the same element");
            return Err(AccountError::QueryInstrumentNotAccepted {
                account: material.id.clone(),
                instrument,
            });
        }
        Ok(Self {
            id,
            book,
            material: material.id.clone(),
            instruments,
        })
    }
}

/// The ergonomic surface is a declaration; desugaring stores one of three
/// separate objects and returns the concrete object that was created.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountDeclaration {
    Material {
        id: MaterialAccountId,
        contract: AccountContract,
    },
    Virtual {
        id: VirtualViewId,
        query: ViewExpression,
        recognizer: RecognizerMapping,
    },
    Book {
        id: BookAccountId,
        book: BookId,
        material: MaterialAccountId,
        instruments: BTreeSet<Instrument>,
    },
}

impl AccountDeclaration {
    pub fn material(id: MaterialAccountId, contract: AccountContract) -> Self {
        Self::Material { id, contract }
    }

    pub fn virtual_view(
        id: VirtualViewId,
        query: ViewExpression,
        recognizer: RecognizerMapping,
    ) -> Self {
        Self::Virtual {
            id,
            query,
            recognizer,
        }
    }

    pub fn book(
        id: BookAccountId,
        book: BookId,
        material: MaterialAccountId,
        instruments: impl IntoIterator<Item = Instrument>,
    ) -> Self {
        Self::Book {
            id,
            book,
            material,
            instruments: instruments.into_iter().collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeclaredAccount {
    Material(MaterialAccount),
    Virtual(VirtualView),
    Book(BookAccount),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccountRegistry {
    materials: BTreeMap<MaterialAccountId, MaterialAccount>,
    virtuals: BTreeMap<VirtualViewId, VirtualView>,
    books: BTreeMap<(BookId, BookAccountId), BookAccount>,
}

impl AccountRegistry {
    pub fn declare(
        &mut self,
        declaration: AccountDeclaration,
    ) -> Result<DeclaredAccount, AccountError> {
        match declaration {
            AccountDeclaration::Material { id, contract } => {
                if self.materials.contains_key(&id) {
                    return Err(AccountError::DuplicateMaterial(id));
                }
                let account = MaterialAccount::try_new(id.clone(), contract)?;
                self.materials.insert(id, account.clone());
                Ok(DeclaredAccount::Material(account))
            }
            AccountDeclaration::Virtual {
                id,
                query,
                recognizer,
            } => {
                if self.virtuals.contains_key(&id) {
                    return Err(AccountError::DuplicateVirtual(id));
                }
                if matches!(&query, ViewExpression::Union(v) | ViewExpression::Intersection(v) if v.is_empty())
                {
                    return Err(AccountError::EmptyQuerySet);
                }
                let books = self
                    .books
                    .keys()
                    .map(|(book, _)| book.clone())
                    .collect::<BTreeSet<_>>();
                query.validate(&self.materials, &books)?;
                if recognizer.source.as_str().is_empty() {
                    return Err(AccountError::EmptyQuery);
                }
                // Revalidate against the registry: a caller cannot construct
                // a recognizer for a material account from another registry.
                let recognizer = RecognizerMapping::try_new(
                    recognizer.source.clone(),
                    recognizer.rules.clone(),
                    &self.materials,
                )?;
                let view = VirtualView {
                    id: id.clone(),
                    query,
                    recognizer,
                };
                self.virtuals.insert(id, view.clone());
                Ok(DeclaredAccount::Virtual(view))
            }
            AccountDeclaration::Book {
                id,
                book,
                material,
                instruments,
            } => {
                let key = (book.clone(), id.clone());
                if self.books.contains_key(&key) {
                    return Err(AccountError::DuplicateBookAccount { book, account: id });
                }
                let material_account = self
                    .materials
                    .get(&material)
                    .ok_or_else(|| AccountError::UnknownMaterial(material.clone()))?;
                let account = BookAccount::try_new(id, book, material_account, instruments)?;
                self.books.insert(key, account.clone());
                Ok(DeclaredAccount::Book(account))
            }
        }
    }

    pub fn material(&self, id: &MaterialAccountId) -> Option<&MaterialAccount> {
        self.materials.get(id)
    }

    pub fn virtual_view(&self, id: &VirtualViewId) -> Option<&VirtualView> {
        self.virtuals.get(id)
    }

    pub fn book_account(&self, book: &BookId, id: &BookAccountId) -> Option<&BookAccount> {
        self.books.get(&(book.clone(), id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id<T: FromId>(value: &str) -> T {
        T::make(value)
    }

    trait FromId: Sized {
        fn make(value: &str) -> Self;
    }

    macro_rules! from_id {
        ($($name:ident),+ $(,)?) => {$ (
            impl FromId for $name {
                fn make(value: &str) -> Self { $name::try_new(value).unwrap() }
            }
        )+ };
    }
    from_id!(
        MaterialAccountId,
        VirtualViewId,
        BookAccountId,
        PartyId,
        BookId
    );

    fn contract() -> AccountContract {
        AccountContract::try_new(
            id::<PartyId>("alice"),
            [Role::Owner, Role::Holder],
            [Instrument::Cash, Instrument::Security],
        )
        .unwrap()
    }

    #[test]
    fn layers_have_nominally_distinct_identities() {
        let material = id::<MaterialAccountId>("checking");
        let virtual_id = id::<VirtualViewId>("checking");
        let book_id = id::<BookAccountId>("checking");
        // Equal spelling does not make these values interchangeable.  This
        // assertion is intentionally type-directed rather than string-based.
        assert_eq!(material.as_str(), virtual_id.as_str());
        assert_eq!(virtual_id.as_str(), book_id.as_str());
        let _: MaterialAccountId = material;
        let _: VirtualViewId = virtual_id;
        let _: BookAccountId = book_id;
    }

    #[test]
    fn contract_rejects_invalid_role_instrument_pairs() {
        let error = AccountContract::try_new(
            id::<PartyId>("issuer"),
            [Role::Issuer],
            [Instrument::Service],
        )
        .unwrap_err();
        assert!(matches!(
            error,
            AccountError::InvalidRoleForInstrument {
                role: Role::Issuer,
                instrument: Instrument::Service
            }
        ));

        let forged = AccountContract {
            party: id("issuer"),
            roles: [Role::Issuer].into_iter().collect(),
            instruments: [Instrument::Service].into_iter().collect(),
        };
        assert!(matches!(
            MaterialAccount::try_new(id("services"), forged),
            Err(AccountError::InvalidRoleForInstrument { .. })
        ));
    }

    #[test]
    fn registry_rejects_duplicate_identities_and_preserves_atomicity() {
        let mut registry = AccountRegistry::default();
        let declaration = AccountDeclaration::material(id("checking"), contract());
        registry.declare(declaration.clone()).unwrap();
        assert!(matches!(
            registry.declare(declaration),
            Err(AccountError::DuplicateMaterial(_))
        ));
        assert_eq!(registry.materials.len(), 1);
    }

    #[test]
    fn virtual_view_requires_known_source_and_valid_query() {
        let mut registry = AccountRegistry::default();
        let source = id::<MaterialAccountId>("checking");
        let recognizer = RecognizerMapping::try_new(
            source.clone(),
            [RecognitionRule {
                instrument: Instrument::Cash,
                role: Role::Holder,
                output: ViewField::Balance,
            }],
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(matches!(recognizer, AccountError::MappingSourceUnknown(_)));
        registry
            .declare(AccountDeclaration::material(source.clone(), contract()))
            .unwrap();
        let bad = AccountDeclaration::virtual_view(
            id("cash"),
            ViewExpression::position(source.clone(), Instrument::Commodity),
            RecognizerMapping::try_new(
                source,
                [RecognitionRule {
                    instrument: Instrument::Cash,
                    role: Role::Holder,
                    output: ViewField::Balance,
                }],
                &registry.materials,
            )
            .unwrap(),
        );
        assert!(matches!(
            registry.declare(bad),
            Err(AccountError::QueryInstrumentNotAccepted { .. })
        ));

        let source = id::<MaterialAccountId>("checking");
        let recognizer = RecognizerMapping::try_new(
            source.clone(),
            [RecognitionRule {
                instrument: Instrument::Cash,
                role: Role::Holder,
                output: ViewField::Balance,
            }],
            &registry.materials,
        )
        .unwrap();
        assert!(matches!(
            registry.declare(AccountDeclaration::virtual_view(
                id("unknown-book"),
                ViewExpression::InBook(id("missing")),
                recognizer,
            )),
            Err(AccountError::UnknownBook(_))
        ));
    }

    #[test]
    fn book_accounts_are_scoped_and_cannot_leak_between_books() {
        let mut registry = AccountRegistry::default();
        let material = id::<MaterialAccountId>("cash");
        registry
            .declare(AccountDeclaration::material(material.clone(), contract()))
            .unwrap();
        let first = id::<BookId>("personal");
        let second = id::<BookId>("company");
        let account = id::<BookAccountId>("cash");
        for book in [first.clone(), second.clone()] {
            registry
                .declare(AccountDeclaration::book(
                    account.clone(),
                    book,
                    material.clone(),
                    [Instrument::Cash],
                ))
                .unwrap();
        }
        assert!(registry.book_account(&first, &account).is_some());
        assert!(registry.book_account(&second, &account).is_some());
        assert_eq!(registry.books.len(), 2);
        let wrong_book = id::<BookId>("tax");
        assert!(registry.book_account(&wrong_book, &account).is_none());
    }

    #[test]
    fn book_account_cannot_accept_instrument_outside_material_contract() {
        let mut registry = AccountRegistry::default();
        let material = id::<MaterialAccountId>("cash");
        registry
            .declare(AccountDeclaration::material(material.clone(), contract()))
            .unwrap();
        let error = registry
            .declare(AccountDeclaration::book(
                id("cash"),
                id("personal"),
                material,
                [Instrument::Commodity],
            ))
            .unwrap_err();
        assert!(matches!(
            error,
            AccountError::QueryInstrumentNotAccepted { .. }
        ));
        assert!(registry.books.is_empty());
    }

    #[test]
    fn declaration_desugars_to_three_separate_objects() {
        let mut registry = AccountRegistry::default();
        let source = id::<MaterialAccountId>("cash");
        let material = registry
            .declare(AccountDeclaration::material(source.clone(), contract()))
            .unwrap();
        assert!(matches!(material, DeclaredAccount::Material(_)));
        let recognizer = RecognizerMapping::try_new(
            source.clone(),
            [RecognitionRule {
                instrument: Instrument::Cash,
                role: Role::Holder,
                output: ViewField::Balance,
            }],
            &registry.materials,
        )
        .unwrap();
        let virtual_view = registry
            .declare(AccountDeclaration::virtual_view(
                id("cash-balance"),
                ViewExpression::position(source.clone(), Instrument::Cash),
                recognizer,
            ))
            .unwrap();
        assert!(matches!(virtual_view, DeclaredAccount::Virtual(_)));
        let book = registry
            .declare(AccountDeclaration::book(
                id("cash"),
                id("personal"),
                source,
                [Instrument::Cash],
            ))
            .unwrap();
        assert!(matches!(book, DeclaredAccount::Book(_)));
        assert_eq!(registry.materials.len(), 1);
        assert_eq!(registry.virtuals.len(), 1);
        assert_eq!(registry.books.len(), 1);
    }
}
