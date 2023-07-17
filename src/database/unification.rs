use std::{collections::HashMap, iter::empty, pin::Pin, rc::Rc};

use crate::database::backtracking::BacktrackingQuery;

use super::{evaluator::VariableName, DBValue, Database};

#[derive(Clone, Debug, PartialEq)]
pub enum GlobPosition {
    Head,
    Tail,
    Middle,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Literal(DBValue),
    Variable(VariableName),
    PatternMatch {
        explicit_values: Vec<Value>,
        is_glob: bool,
        glob_position: GlobPosition,
    },
}

pub type RelationID = String;

pub type Bindings = HashMap<VariableName, Value>;
pub fn chain_hashmap<K: Clone + Eq + std::hash::Hash, V: Clone>(
    a: HashMap<K, V>,
    b: HashMap<K, V>,
) -> HashMap<K, V> {
    a.into_iter().chain(b).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum EqOp {
    GreaterThan,
    EqualTo,
    LessThan,
    LessThanOrEqualTo,
    GreaterThanOrEqualTo,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    Relation(RelationID, Vec<Value>),
    Unification(Vec<Value>, Vec<Value>),
    Comparison(EqOp, Value, Value),
    Not(Box<Constraint>),
    Alternatives(Vec<Constraint>),
    Intersections(Vec<Constraint>),
}

impl Constraint {
    pub fn new_relation(vs: Vec<Value>) -> Result<Self, String> {
        match vs.get(1) {
            Some(Value::Literal(DBValue::RelationID(rel))) => {
                let mut vs = vs.clone();
                vs.remove(1);
                Ok(Constraint::Relation(rel.to_uppercase(), vs))
            }
            Some(v) => Err(format!(
                "Expected second term in relation to be a valid relation ID, not {:?}",
                v
            )),
            None => {
                Err("Not enough terms in relation to construct a meaningful relation. ".to_string())
            }
        }
    }
}

pub fn unify_wrap(
    av: &Vec<Value>,
    bv: &Vec<Value>,
    bindings: Rc<Bindings>,
) -> Option<Rc<Bindings>> {
    let inner = (*bindings).clone();

    lax_unify(av, bv, inner).ok().map(|x| Rc::new(x))
}

pub fn lax_unify_wrap(
    av: &Vec<Value>,
    bv: &Vec<Value>,
    bindings: Rc<Bindings>,
) -> Result<Rc<Bindings>, Rc<Bindings>> {
    let inner = (*bindings).clone();
    lax_unify(av, bv, inner).map_or_else(|x| Err(Rc::new(x)), |x| Ok(Rc::new(x)))
}

pub fn lax_unify(
    av: &Vec<Value>,
    bv: &Vec<Value>,
    bindings: Bindings,
) -> Result<Bindings, Bindings> {
    let mut new_bindings = bindings.clone();
    for (i, j) in av.into_iter().zip(bv) {
        use Value::*;
        match (i, j) {
            (Literal(x), Literal(y)) => {
                if x == y {
                    continue;
                } else {
                    return Err(new_bindings);
                }
            }
            (Variable(x), vy @ Literal(_)) => {
                if let Some(vx) = new_bindings.get(x) {
                    let partials = lax_unify(&vec![vx.clone()], &vec![vy.clone()], new_bindings);
                    if let Ok(binds) = partials {
                        new_bindings = binds;
                    } else {
                        return partials;
                    }
                } else {
                    new_bindings.insert(x.clone(), vy.clone());
                }
            }
            (vx @ Literal(_), Variable(y)) => {
                if let Some(vy) = new_bindings.get(y) {
                    let partials = lax_unify(&vec![vx.clone()], &vec![vy.clone()], new_bindings);
                    if let Ok(binds) = partials {
                        new_bindings = binds;
                    } else {
                        return partials;
                    }
                } else {
                    new_bindings.insert(y.clone(), vx.clone());
                }
            }
            (Variable(x), Variable(y)) => {
                let vy = Variable(y.clone());
                new_bindings.insert(x.clone(), vy);
            }
            (
                Literal(DBValue::List(list)),
                PatternMatch {
                    explicit_values,
                    is_glob,
                    glob_position,
                },
            ) => {
                let partials = unify_pattern_match(
                    list,
                    explicit_values,
                    is_glob,
                    glob_position,
                    new_bindings,
                );
                if let Ok(binds) = partials {
                    new_bindings = binds;
                } else {
                    return partials;
                }
            }
            (
                PatternMatch {
                    explicit_values,
                    is_glob,
                    glob_position,
                },
                Literal(DBValue::List(list)),
            ) => {
                let partials = unify_pattern_match(
                    list,
                    explicit_values,
                    is_glob,
                    glob_position,
                    new_bindings,
                );
                if let Ok(binds) = partials {
                    new_bindings = binds;
                } else {
                    return partials;
                }
            }
            _ => {
                return Err(new_bindings);
            }
        }
    }
    Ok(new_bindings)
}

pub fn unify_pattern_match(
    list: &Vec<DBValue>,
    explicit_values: &Vec<Value>,
    is_glob: &bool,
    glob_position: &GlobPosition,
    new_bindings: Bindings,
) -> Result<Bindings, Bindings> {
    use Value::*;
    let partials = if !is_glob {
        lax_unify(
            &explicit_values,
            &list.clone().into_iter().map(|x| Literal(x)).collect(),
            new_bindings,
        )
    } else {
        let n = explicit_values.len();
        match glob_position {
            GlobPosition::Head => lax_unify(
                &explicit_values,
                &list
                    .clone()
                    .into_iter()
                    .take(n)
                    .map(|x| Literal(x))
                    .collect(),
                new_bindings,
            ),
            GlobPosition::Tail => lax_unify(
                &explicit_values,
                &list
                    .clone()
                    .into_iter()
                    .rev()
                    .take(n)
                    .map(|x| Literal(x))
                    .collect(),
                new_bindings,
            ),
            GlobPosition::Middle => {
                // Find the complete match, or largest incomplete match, at any position in the middle of the array
                let list: Vec<Value> = list.clone().into_iter().map(|x| Literal(x)).collect();
                let mut output = Err(HashMap::new());
                for i in 0..list.len() {
                    let partials = lax_unify(
                        &explicit_values,
                        &list[i..i + n].to_vec(),
                        new_bindings.clone(),
                    );
                    if partials.is_ok() {
                        output = partials;
                    } else if output.is_err() {
                        let len1 = partials.as_ref().map_err(|x| x.len()).unwrap_err();
                        let len2 = output.as_ref().map_err(|x| x.len()).unwrap_err();
                        if len1 > len2 {
                            output = partials;
                        }
                    }
                }
                output
            }
        }
    };
    partials
}

pub fn unify_compare(op: &EqOp, a: &Value, b: &Value, bindings: Rc<Bindings>) -> bool {
    use Value::*;
    match (a, b) {
        (Literal(a), Literal(b)) => match op {
            EqOp::GreaterThan => a > b,
            EqOp::EqualTo => a == b,
            EqOp::LessThan => a < b,
            EqOp::LessThanOrEqualTo => a <= b,
            EqOp::GreaterThanOrEqualTo => a >= b,
        },
        (Variable(x), b @ Literal(_)) => {
            if let Some(xval) = bindings.get(x) {
                unify_compare(op, xval, b, bindings.clone())
            } else {
                true
            }
        }
        (a @ Literal(_), Variable(y)) => {
            if let Some(yval) = bindings.get(y) {
                unify_compare(op, a, yval, bindings.clone())
            } else {
                true
            }
        }
        (Variable(x), Variable(y)) => match (bindings.get(x), bindings.get(y)) {
            (Some(xval), Some(yval)) => unify_compare(op, xval, yval, bindings.clone()),
            (None, Some(_)) => true,
            (Some(_), None) => true,
            (None, None) => true,
        },
        _ => false,
    }
}

type BindingsIterator<'a> = Box<dyn Iterator<Item = Result<Rc<Bindings>, Rc<Bindings>>> + 'a>;

pub struct InnerFactPossibilitiesIter {
    pub database: Rc<Database>,
    pub id: RelationID,
    pub bindings: Rc<Bindings>,
    pub tokens: Vec<Value>,
    fact_index: usize,
}

impl InnerFactPossibilitiesIter {
    pub fn new(
        id: RelationID,
        tokens: Vec<Value>,
        database: Rc<Database>,
        bindings: Rc<Bindings>,
    ) -> Self {
        Self {
            id,
            database,
            bindings,
            tokens,
            fact_index: 0,
        }
    }
}

impl Iterator for InnerFactPossibilitiesIter {
    type Item = Result<Rc<Bindings>, Rc<Bindings>>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(facts) = self.database.facts.get(&self.id) {
            if self.fact_index < facts.len() {
                let fact = &facts[self.fact_index];
                let fact_tokens = fact.iter().map(|x| Value::Literal(x.clone())).collect();
                self.fact_index += 1;
                println!("Found next possible binding for constraint given previous bindings");
                Some(lax_unify_wrap(
                    &self.tokens,
                    &fact_tokens,
                    self.bindings.clone(),
                ))
            } else {
                None
            }
        } else {
            None
        }
    }
}

pub struct InnerBacktrackingQueryIter<'a> {
    pub database: Rc<Database>,
    pub id: RelationID,
    pub bindings: Rc<Bindings>,
    pub tokens: Vec<Value>,
    inner_iterator: BindingsIterator<'a>,
    query_index: usize,
}

impl<'a> InnerBacktrackingQueryIter<'a> {
    pub fn new(
        id: RelationID,
        tokens: Vec<Value>,
        database: Rc<Database>,
        bindings: Rc<Bindings>,
        constraints: &'a [Constraint],
        params: Vec<Value>,
    ) -> Self {
        let mut res = Self {
            id,
            database: database.clone(),
            bindings,
            tokens,
            inner_iterator: Box::new(empty()),
            query_index: 0,
        };
        let db = database.clone();
        res.inner_iterator =
            if let Some(args) = unify_wrap(&res.tokens, &params, res.bindings.clone()) {
                Box::new(BacktrackingQuery::new(constraints, db, args.clone()).map(|x| Ok(x)))
            } else {
                Box::new(empty())
            };
        res
    }
}

impl<'a> Iterator for InnerBacktrackingQueryIter<'a> {
    type Item = Result<Rc<Bindings>, Rc<Bindings>>;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner_iterator.next()
    }
}

pub struct PossibleBindings<'b> {
    pub constraint: &'b Constraint,
    pub database: Rc<Database>,
    pub bindings: Rc<Bindings>,
    current_fact_possibilities: BindingsIterator<'b>,
    current_rule_possibilities: BindingsIterator<'b>,
    done: bool,
}

impl<'b> PossibleBindings<'b> {
    pub fn new(constraint: &'b Constraint, database: Rc<Database>, bindings: Rc<Bindings>) -> Self {
        Self {
            constraint,
            database,
            bindings,
            current_fact_possibilities: Box::new(empty()),
            current_rule_possibilities: Box::new(empty()),
            done: false,
        }
    }
    pub fn new_with_bindings(
        constraint: &'b Constraint,
        database: Rc<Database>,
        bindings: Rc<Bindings>,
        possibilities: Vec<Rc<Bindings>>,
    ) -> Self {
        Self {
            constraint,
            database,
            bindings,
            current_fact_possibilities: Box::new(possibilities.into_iter().map(|x| Ok(x))),
            current_rule_possibilities: Box::new(empty()),
            done: true,
        }
    }
}

impl<'b> Iterator for PossibleBindings<'b> {
    type Item = Result<Rc<Bindings>, Rc<Bindings>>;

    fn next(&mut self) -> Option<Self::Item> {
        use Constraint::*;
        if let Some(binding) = self.current_fact_possibilities.next() {
            Some(binding)
        } else if let Some(binding) = self.current_rule_possibilities.next() {
            Some(binding)
        } else if !self.done {
            match self.constraint {
                Relation(id, tokens) => {
                    self.current_fact_possibilities = Box::new(InnerFactPossibilitiesIter::new(
                        id.to_string(),
                        tokens.to_vec(),
                        self.database.clone(),
                        self.bindings.clone(),
                    ));
                    /*let val = self.database.rules.get(id);
                    if let Some((params, constraints)) = val {
                        self.current_rule_possibilities =
                            Box::new(InnerBacktrackingQueryIter::new(
                                id.to_string(),
                                tokens.to_vec(),
                                self.database.clone(),
                                self.bindings.clone(),
                                constraints,
                                params.clone(),
                            ));
                    }*/
                }

                Comparison(op, a, b) => {
                    if unify_compare(&op, &a, &b, self.bindings.clone()) {
                        self.current_fact_possibilities =
                            Box::new(vec![Ok(self.bindings.clone())].into_iter());
                    } else {
                        self.current_fact_possibilities = Box::new(empty());
                    }
                }

                Unification(avs, bvs) => {
                    if let Some(new_bindings) = unify_wrap(avs, bvs, self.bindings.clone()) {
                        self.current_fact_possibilities =
                            Box::new(vec![Ok(new_bindings)].into_iter());
                    } else {
                        self.current_fact_possibilities = Box::new(empty());
                    }
                }

                Not(constraint) => {
                    let shadow_binding = self.bindings.clone();
                    let shadow_database = self.database.clone();
                    self.current_fact_possibilities = Box::new(
                        PossibleBindings::new(
                            constraint,
                            shadow_database.clone(),
                            shadow_binding.clone(),
                        )
                        .map(|x| x.map_or_else(|x| Ok(x), |x| Err(x))),
                    );
                }

                Alternatives(constraints) => {
                    let shadow_binding = self.bindings.clone();
                    let shadow_database = self.database.clone();
                    let possibilities = constraints.iter().flat_map(move |constraint| {
                        PossibleBindings::new(
                            constraint,
                            shadow_database.clone(),
                            shadow_binding.clone(),
                        )
                    });
                    self.current_fact_possibilities = Box::new(possibilities);
                }

                Intersections(constraints) => {
                    let possible_binds = BacktrackingQuery::new(
                        constraints,
                        self.database.clone(),
                        self.bindings.clone(),
                    )
                    .map(|x| Ok(x));
                    self.current_rule_possibilities = Box::new(possible_binds);
                }
            }
            self.done = true;
            self.current_fact_possibilities.next()
        } else {
            None
        }
    }
}