use crate::package::Package;
use std::collections::HashMap;

#[derive(Debug, Default, PartialEq, Clone)]
pub struct RepositoryDatabase {
    pub(crate) name: String,
    pub(crate) packages: HashMap<String, Vec<Package>>,
}
