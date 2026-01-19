use diesel::prelude::*;

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::products)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Product {
    pub id: i32,
    pub product_name: String,
    pub product_value: i32,
}

#[derive(Insertable)]
#[diesel(table_name = crate::schema::products)]
pub struct NewProduct<'a> {
    pub product_name: &'a str,
    pub product_value: &'a i32,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::taxes)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Tax {
    pub id: i32,
    pub state_name: String,
    pub year: i32,
    pub percent: i32,
    pub product_id: i32,
}
