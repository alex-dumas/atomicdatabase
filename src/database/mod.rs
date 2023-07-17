pub mod backtracking;
pub mod evaluator;
pub mod unification;

use uuid;

use std::collections::HashMap;

use num_bigint::{BigInt, BigUint, ToBigUint};

use self::unification::{Constraint, RelationID, Value};

#[derive(Clone, Debug)]
pub enum DBValue {
    Text(String),
    Number(BigInt),
    Float(BigInt, BigUint),
    RelationID(String),
    List(Vec<DBValue>),
}

impl PartialEq for DBValue {
    fn eq(&self, other: &Self) -> bool {
        use DBValue::*;
        let zero = 0.to_biguint().unwrap();
        match (self, other) {
            (&Text(ref s1), &Text(ref s2)) => s1 == s2,
            (&Number(ref n1), &Number(ref n2)) => n1 == n2,
            (&Number(ref n1), &Float(ref n2, ref n2d)) => *n2d == zero && n1 == n2,
            (&Float(ref n1, ref n1d), &Number(ref n2)) => *n1d == zero && n1 == n2,
            (&Float(ref n1, ref n1d), &Float(ref n2, ref n2d)) => n1d == n2d && n1 == n2,
            (&RelationID(ref r1), &RelationID(ref r2)) => r1.to_uppercase() == r2.to_uppercase(),
            _ => false,
        }
    }
}

impl Eq for DBValue {}

impl PartialOrd for DBValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        use DBValue::*;
        let zero = 0.to_biguint().unwrap();
        match (self, other) {
            (&Text(ref s1), &Text(ref s2)) => Some(s1.cmp(s2)),
            (&Number(ref n1), &Number(ref n2)) => Some(n1.cmp(n2)),
            (&Number(ref n1), &Float(ref n2, ref n2d)) => {
                if n1 == n2 {
                    Some(zero.cmp(n2d))
                } else {
                    Some(n1.cmp(n2))
                }
            }
            (&Float(ref n1, ref n1d), &Number(ref n2)) => {
                if n1 == n2 {
                    Some(zero.cmp(n1d))
                } else {
                    Some(n1.cmp(n2))
                }
            }
            (&Float(ref n1, ref n1d), &Float(ref n2, ref n2d)) => {
                if n1 == n2 {
                    Some(n1d.cmp(n2d))
                } else {
                    Some(n1.cmp(n2))
                }
            }
            _ => None,
        }
    }
}

pub struct Database {
    facts: HashMap<RelationID, Vec<Vec<DBValue>>>,
    rules: HashMap<RelationID, (Vec<Value>, Vec<Constraint>)>,
}

impl Database {
    pub fn new() -> Self {
        Database {
            facts: HashMap::new(),
            rules: HashMap::new(),
        }
    }

    pub fn insert_rule(&mut self, id: RelationID, args: Vec<Value>, constraints: Vec<Constraint>) {
        self.rules.insert(id.to_uppercase(), (args, constraints));
    }

    pub fn insert_fact(&mut self, vs: Vec<DBValue>) {
        match vs.get(1) {
            Some(DBValue::RelationID(rel)) => {
                let rel = rel.to_uppercase();
                let mut vs = vs.clone();
                vs.remove(1);
                if let Some(rels) = self.facts.get_mut(&rel) {
                    rels.push(vs);
                } else {
                    self.facts.insert(rel, vec![vs]);
                }
            }
            Some(v) => panic!(
                "Expected second term in database relation to be a valid relation ID, not {:?}. ",
                v
            ),
            None => {
                panic!("Not enough terms in database relation to construct a meaningful relation. ")
            }
        }
    }
}